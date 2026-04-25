/**
 * Shared test fixtures for x11-web e2e tests.
 *
 * Provides Docker container lifecycle management (backend + sidecar + frontend)
 * and common utility functions used across all test files.
 *
 * Usage in test files:
 *   import { test, expect } from "./fixtures";
 */

import { exec, execSync, spawn } from "node:child_process";
import * as fs from "node:fs";
import * as http from "node:http";
import * as path from "node:path";
import * as os from "node:os";
import {
	type Locator,
	type Page,
	test as base,
	expect,
} from "@playwright/test";
import type { StartedTestContainer } from "testcontainers";
import { GenericContainer, Wait } from "testcontainers";

const PROJECT_ROOT = path.resolve(import.meta.dirname, "../..");
const E2E_DIR = path.resolve(import.meta.dirname, "..");
const FRONTEND_DIR = path.join(PROJECT_ROOT, "frontend");
const SCRIPTS_DIR = path.join(E2E_DIR, "scripts");
const SERVE_BIN = path.join(E2E_DIR, "node_modules", ".bin", "serve");

// ---------------------------------------------------------------------------
// Container state (shared across all tests in a single worker)
// ---------------------------------------------------------------------------
//
// Playwright respawns the worker process after each test failure. Without
// container reuse this would burn one Docker network + container trio per
// failure and exhaust the subnet pool within a couple of dozen failures.
// We use:
//   - A FIXED-NAME Docker network created idempotently via `docker network`
//     (testcontainers' `Network` class generates UUIDs and can't be reused).
//   - `.withReuse()` on each container so the second worker's setup attaches
//     to the existing one instead of building/starting a fresh copy.
//   - A FIXED frontend port + lockfile so workers reuse a single `serve`
//     process rather than spawning one per worker.
const SHARED_NETWORK = "x11web-shared-network";
const STATE_FILE = path.join(os.tmpdir(), "x11web-test-state.json");
type SharedState = { backendPort: number; frontendPort: number };

let backendContainer: StartedTestContainer;
let sidecarContainer: StartedTestContainer;
let frontendPort: number;
let backendPort: number;
let setupDone = false;
let setupPromise: Promise<void> | null = null;

async function ensureSetup() {
	if (setupDone) return;
	if (setupPromise) return setupPromise;
	setupPromise = doSetup();
	return setupPromise;
}

function ensureSharedNetwork(): void {
	try {
		execSync(`docker network inspect ${SHARED_NETWORK}`, { stdio: "pipe" });
	} catch {
		execSync(`docker network create ${SHARED_NETWORK}`, { stdio: "pipe" });
	}
}

async function doSetup() {
	ensureSharedNetwork();

	backendContainer = await GenericContainer.fromDockerfile(
		PROJECT_ROOT,
		"Dockerfile.backend",
	)
		.build("x11-web-backend-test", { deleteOnExit: false })
		.then((image) =>
			image
				.withNetworkMode(SHARED_NETWORK)
				.withNetworkAliases("backend")
				.withExposedPorts(3001)
				.withWaitStrategy(
					Wait.forHttp("/health", 3001).forStatusCode(200),
				)
				.withReuse()
				.start(),
		);

	backendPort = backendContainer.getMappedPort(3001);
	console.log(`Backend running at localhost:${backendPort}`);

	sidecarContainer = await GenericContainer.fromDockerfile(
		PROJECT_ROOT,
		"Dockerfile.sidecar",
	)
		.build("x11-web-sidecar-test", { deleteOnExit: false })
		.then((image) =>
			image
				.withNetworkMode(SHARED_NETWORK)
				.withNetworkAliases("sidecar")
				.withHostname("x11web")
				// Privileged mode lets apps that use Linux user-namespaces for
				// sandboxing (browsers, etc.) work the same as on a normal desktop.
				// This is a container environment requirement, not app-specific.
				.withPrivilegedMode()
				.withEnvironment({
					BACKEND_URL: "ws://backend:3001/ws/sidecar",
					SIDECAR_NAME: "test-sidecar",
					DISPLAY_NUMBER: "99",
					RUST_LOG: "info",
					NO_AT_BRIDGE: "1",
				})
				.withWaitStrategy(Wait.forLogMessage(/Connected to backend/))
				.withReuse()
				.start(),
		);

	console.log("Sidecar connected to backend");

	frontendPort = await ensureFrontendServer(backendPort);
	console.log(`Frontend running at http://localhost:${frontendPort}`);
	setupDone = true;
}

/**
 * Build the frontend (idempotent — the build dir survives) and ensure a
 * `serve` process is running. If a previous worker started one and it's
 * still alive, reuse its port via the lockfile. Otherwise build + spawn a
 * fresh one and persist the port for future workers.
 */
async function ensureFrontendServer(backend: number): Promise<number> {
	if (fs.existsSync(STATE_FILE)) {
		try {
			const state = JSON.parse(fs.readFileSync(STATE_FILE, "utf8")) as SharedState;
			const res = await fetch(`http://localhost:${state.frontendPort}`).catch(() => null);
			if (res?.ok && state.backendPort === backend) {
				return state.frontendPort;
			}
		} catch {
			// stale or malformed; fall through to fresh build
		}
	}

	const wsUrl = `ws://localhost:${backend}/ws/frontend`;
	await new Promise<void>((resolve, reject) => {
		exec(
			`VITE_WS_URL=${wsUrl} pnpm run build`,
			{ cwd: FRONTEND_DIR },
			(error, _stdout, stderr) => {
				if (error) {
					console.error("Frontend build failed:", stderr);
					reject(error);
				} else {
					resolve();
				}
			},
		);
	});

	const port = await findFreePort();
	// Detach so the serve process survives across worker restarts. The
	// global-teardown hook kills it via the `pkill -f 'serve dist -l'`
	// pattern at the end of the run.
	const child = spawn(SERVE_BIN, ["dist", "-l", `${port}`, "--no-clipboard"], {
		cwd: FRONTEND_DIR,
		detached: true,
		stdio: "ignore",
	});
	child.unref();
	await new Promise<void>((resolve, reject) => {
		const timeout = setTimeout(() => {
			clearInterval(check);
			reject(new Error("Frontend server failed to start within 30s"));
		}, 30_000);
		const check = setInterval(async () => {
			try {
				const res = await fetch(`http://localhost:${port}`);
				if (res.ok) {
					clearInterval(check);
					clearTimeout(timeout);
					resolve();
				}
			} catch {
				// Not ready yet
			}
		}, 200);
	});

	const state: SharedState = { backendPort: backend, frontendPort: port };
	fs.writeFileSync(STATE_FILE, JSON.stringify(state));
	return port;
}

async function teardownAll() {
	// The frontend serve process and the reused containers/network are
	// owned by the test session, not this worker. Final cleanup happens
	// in global-teardown.ts via the testcontainers label and pkill.
	setupDone = false;
	setupPromise = null;
}

function findFreePort(): Promise<number> {
	return new Promise((resolve) => {
		const server = http.createServer();
		server.listen(0, () => {
			const port = (server.address() as { port: number }).port;
			server.close(() => resolve(port));
		});
	});
}

// ---------------------------------------------------------------------------
// Custom test fixture
// ---------------------------------------------------------------------------
export type X11Fixtures = {
	sidecarContainer: StartedTestContainer;
	frontendUrl: string;
};

export const test = base.extend<{}, X11Fixtures>({
	sidecarContainer: [
		async ({}, use) => {
			await ensureSetup();
			await use(sidecarContainer);
		},
		{ scope: "worker" },
	],
	frontendUrl: [
		async ({}, use) => {
			await ensureSetup();
			await use(`http://localhost:${frontendPort}`);
		},
		{ scope: "worker" },
	],
});

// Re-export expect for convenience
export { expect };

// ---------------------------------------------------------------------------
// Utility functions (used by test files)
// ---------------------------------------------------------------------------

/** Spawn an app via the frontend UI and return the new window frame locator. */
export async function spawnApp(
	page: Page,
	args = "",
	command = "xeyes",
	/** Timeout in ms for the window frame to appear. */
	windowTimeout = 15_000,
): Promise<Locator> {
	const windowFrames = page.locator('[data-testid="window-frame"]');
	const countBefore = await windowFrames.count();

	await page.locator('[data-testid="spawn-button"]').click();
	if (command !== "xeyes") {
		await page.locator('input[placeholder="command"]').fill(command);
	}
	if (args) {
		await page.locator('input[placeholder="args"]').fill(args);
	}
	await expect(
		page.locator("button", { hasText: "Spawn" }),
	).toBeEnabled({ timeout: 30_000 });
	await page.locator("button", { hasText: "Spawn" }).click();

	await expect(windowFrames).toHaveCount(countBefore + 1, {
		timeout: windowTimeout,
	});
	return windowFrames.nth(countBefore);
}

export async function waitForDock(page: Page) {
	const dock = page.locator('[data-testid="dock"]');
	await expect(dock).toBeVisible({ timeout: 15_000 });
	await expect(page.locator('[data-testid="spawn-button"]')).toBeVisible({
		timeout: 15_000,
	});
}

export async function countNonBlackPixels(canvas: Locator): Promise<number> {
	return canvas.evaluate((el: HTMLCanvasElement) => {
		const ctx = el.getContext("2d");
		if (!ctx) return 0;
		const d = ctx.getImageData(0, 0, el.width, el.height);
		let n = 0;
		for (let i = 0; i < d.data.length; i += 4) {
			if (d.data[i] || d.data[i + 1] || d.data[i + 2]) n++;
		}
		return n;
	});
}

export async function hasRenderedContent(canvas: Locator): Promise<boolean> {
	return canvas.evaluate((el: HTMLCanvasElement) => {
		const ctx = el.getContext("2d");
		if (!ctx) return false;
		const d = ctx.getImageData(0, 0, el.width, el.height);
		const colors = new Set<number>();
		for (let i = 0; i < d.data.length; i += 4) {
			const c = (d.data[i] << 16) | (d.data[i + 1] << 8) | d.data[i + 2];
			colors.add(c);
			if (colors.size >= 2) return true;
		}
		return false;
	});
}

export async function canvasPixelHash(canvas: Locator): Promise<string> {
	return canvas.evaluate((el: HTMLCanvasElement) => {
		const ctx = el.getContext("2d");
		if (!ctx) return "";
		const d = ctx.getImageData(0, 0, el.width, el.height);
		let h = 0x811c9dc5 | 0;
		for (let i = 0; i < d.data.length; i += 16) {
			h = (h ^ d.data[i]) >>> 0;
			h = Math.imul(h, 0x01000193) >>> 0;
		}
		return `${el.width}x${el.height}:${h.toString(16)}`;
	});
}

export async function waitForCanvasStable(
	canvas: Locator,
	{
		stableMs = 1000,
		pollMs = 200,
		totalTimeoutMs = 15_000,
	}: { stableMs?: number; pollMs?: number; totalTimeoutMs?: number } = {},
): Promise<void> {
	const start = Date.now();
	let lastHash = "";
	let stableSince = 0;
	while (Date.now() - start < totalTimeoutMs) {
		const hash = await canvasPixelHash(canvas);
		const now = Date.now();
		if (hash === lastHash && hash !== "") {
			if (stableSince === 0) stableSince = now;
			if (now - stableSince >= stableMs) return;
		} else {
			lastHash = hash;
			stableSince = 0;
		}
		await new Promise((r) => setTimeout(r, pollMs));
	}
}

/**
 * Run a Python script (loaded from `e2e/scripts/<name>`) inside the sidecar
 * container. The script is staged into `/tmp/<name>` via a heredoc so the
 * container doesn't need a bind mount.
 *
 * `env` is rendered as a `KEY=VAL` prefix (most callers want `DISPLAY=:99`).
 */
export async function runPythonScript(
	container: StartedTestContainer,
	scriptName: string,
	{
		env = {},
		args = [],
	}: { env?: Record<string, string>; args?: string[] } = {},
): Promise<{ output: string; exitCode: number }> {
	const script = fs.readFileSync(path.join(SCRIPTS_DIR, scriptName), "utf8");
	const tmpPath = `/tmp/${scriptName}`;
	await container.exec([
		"bash",
		"-c",
		`cat > ${tmpPath} << 'PYEOF'\n${script}\nPYEOF`,
	]);
	const envPrefix = Object.entries(env)
		.map(([k, v]) => `${k}=${v}`)
		.join(" ");
	const argsStr = args.join(" ");
	const cmd = [envPrefix, "python3", tmpPath, argsStr, "2>&1"]
		.filter(Boolean)
		.join(" ");
	return container.exec(["bash", "-c", cmd]);
}

/** Kill all spawned X11 apps between tests. */
export async function cleanupApps(
	container: StartedTestContainer,
): Promise<void> {
	await container
		?.exec([
			"bash",
			"-c",
			"pkill -9 -f 'xeyes|xterm|xlogo|xclock|xmessage|zenity|firefox|vim|gimp|gtk3-demo|gnome-calculator|qpdfview|libreoffice|soffice|emacs|gnome-text-editor|dbusmenu-test' 2>/dev/null; true",
		])
		.catch(() => {});
	await container
		?.exec([
			"bash",
			"-c",
			"rm -rf /root/.mozilla /root/.cache/mozilla 2>/dev/null; true",
		])
		.catch(() => {});
	await new Promise((r) => setTimeout(r, 2000));
}

// Register cleanup on process termination signals.
// `beforeExit` is unreliable in test runners — use SIGINT/SIGTERM instead.
for (const signal of ["SIGINT", "SIGTERM"] as const) {
	process.on(signal, () => {
		teardownAll()
			.catch(() => {})
			.finally(() => process.exit());
	});
}
