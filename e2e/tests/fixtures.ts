/**
 * Shared test fixtures for x11-web e2e tests.
 *
 * Provides Docker container lifecycle management (backend + sidecar + frontend)
 * and common utility functions used across all test files.
 *
 * Usage in test files:
 *   import { test, expect } from "./fixtures";
 */

import { type ChildProcess, exec } from "node:child_process";
import * as http from "node:http";
import * as path from "node:path";
import {
	type Locator,
	type Page,
	test as base,
	expect,
} from "@playwright/test";
import type { StartedNetwork, StartedTestContainer } from "testcontainers";
import { GenericContainer, Network, Wait } from "testcontainers";

const PROJECT_ROOT = path.resolve(import.meta.dirname, "../..");
const E2E_DIR = path.resolve(import.meta.dirname, "..");
const FRONTEND_DIR = path.join(PROJECT_ROOT, "frontend");
const SERVE_BIN = path.join(E2E_DIR, "node_modules", ".bin", "serve");

// ---------------------------------------------------------------------------
// Container state (shared across all tests in a single worker)
// ---------------------------------------------------------------------------
let network: StartedNetwork;
let backendContainer: StartedTestContainer;
let sidecarContainer: StartedTestContainer;
let frontendServer: ChildProcess;
let frontendPort: number;
let backendPort: number;
let setupDone = false;

async function ensureSetup() {
	if (setupDone) return;

	network = await new Network().start();

	backendContainer = await GenericContainer.fromDockerfile(
		PROJECT_ROOT,
		"Dockerfile.backend",
	)
		.build("x11-web-backend-test", { deleteOnExit: false })
		.then((image) =>
			image
				.withNetwork(network)
				.withNetworkAliases("backend")
				.withExposedPorts(3001)
				.withWaitStrategy(
					Wait.forHttp("/health", 3001).forStatusCode(200),
				)
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
				.withNetwork(network)
				.withNetworkAliases("sidecar")
				.withHostname("x11web")
				.withEnvironment({
					BACKEND_URL: "ws://backend:3001/ws/sidecar",
					SIDECAR_NAME: "test-sidecar",
					DISPLAY_NUMBER: "99",
					RUST_LOG: "info",
					NO_AT_BRIDGE: "1",
					MOZ_USE_XINPUT2: "1",
					// Firefox sandbox requires user namespaces, unavailable in Docker
					MOZ_DISABLE_CONTENT_SANDBOX: "1",
					MOZ_DISABLE_GMP_SANDBOX: "1",
				})
				.withWaitStrategy(Wait.forLogMessage(/Connected to backend/))
				.start(),
		);

	console.log("Sidecar connected to backend");

	const wsUrl = `ws://localhost:${backendPort}/ws/frontend`;
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

	frontendPort = await findFreePort();
	await new Promise<void>((resolve, reject) => {
		frontendServer = exec(
			`${SERVE_BIN} dist -l ${frontendPort} --no-clipboard`,
			{ cwd: FRONTEND_DIR },
		);
		const timeout = setTimeout(() => {
			clearInterval(check);
			reject(new Error("Frontend server failed to start within 30s"));
		}, 30_000);
		const check = setInterval(async () => {
			try {
				const res = await fetch(`http://localhost:${frontendPort}`);
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

	console.log(`Frontend running at http://localhost:${frontendPort}`);
	setupDone = true;
}

async function teardownAll() {
	if (frontendServer?.pid && !frontendServer.killed) {
		frontendServer.kill("SIGTERM");
		// Give it a moment, then force-kill if still alive
		await new Promise((r) => setTimeout(r, 500));
		if (!frontendServer.killed) frontendServer.kill("SIGKILL");
	}
	await sidecarContainer?.stop().catch(() => {});
	await backendContainer?.stop().catch(() => {});
	await network?.stop().catch(() => {});
	setupDone = false;
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

export const test = base.extend<X11Fixtures>({
	sidecarContainer: async ({}, use) => {
		await ensureSetup();
		await use(sidecarContainer);
	},
	frontendUrl: async ({}, use) => {
		await ensureSetup();
		await use(`http://localhost:${frontendPort}`);
	},
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
	/** Timeout in ms for the window frame to appear (default 15s, use longer for Firefox/GIMP). */
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
