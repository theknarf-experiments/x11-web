/**
 * Shared test fixtures for x11-web e2e tests.
 *
 * Provides Docker container lifecycle management (backend + sidecar + frontend)
 * and common utility functions used across all test files.
 *
 * Usage in test files:
 *   import { test, expect } from "./fixtures";
 */

import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import {
	test as base,
	expect,
	type Locator,
	type Page,
} from "@playwright/test";
import type { StartedTestContainer } from "testcontainers";
import { GenericContainer, Wait } from "testcontainers";

const PROJECT_ROOT = path.resolve(import.meta.dirname, "../..");
const E2E_DIR = path.resolve(import.meta.dirname, "..");
const FRONTEND_DIR = path.join(PROJECT_ROOT, "frontend");
const SCRIPTS_DIR = path.join(E2E_DIR, "scripts");

// ---------------------------------------------------------------------------
// Per-worker container state
// ---------------------------------------------------------------------------
//
// Each Playwright worker process gets its own (backend, sidecar, frontend
// `serve`) trio so tests can run in parallel without colliding on the X
// server, atom table, dock UI, etc. The frontend build itself is shared
// across workers (produced once by global-setup.ts); the runtime backend
// WS URL is passed per-page via a `?ws=` query param baked into
// `frontendUrl` below — see `useBackendSocket.ts`.
//
// Every worker uses its own private Docker network (so the alias `backend`
// resolves to *its* backend) named after the Playwright parallel-slot
// index. The network name and `.withReuse()` keys MUST be stable across
// worker respawns (Playwright respawns workers on test failure) —
// otherwise each respawn would leak the previous network/containers.
//
// `TEST_WORKER_INDEX` is *not* stable: it's a monotonically increasing
// counter that goes up every time a worker process spawns, including
// respawns. `TEST_PARALLEL_INDEX` is the parallel-slot index in
// `[0, workers)`, which IS stable across respawns of the same slot.
// Using the wrong one was the leak: with `workers: 2` and many failures
// we'd see 20+ distinct indexes, each allocating its own 3-container set.
const WORKER_INDEX = process.env.TEST_PARALLEL_INDEX ?? "0";
const WORKER_NETWORK = `x11web-worker-${WORKER_INDEX}`;

let backendContainer: StartedTestContainer;
let sidecarContainer: StartedTestContainer;
let mockOidcContainer: StartedTestContainer;
let mockOidcPort: number;
let backendPort: number;
let setupDone = false;
let setupPromise: Promise<void> | null = null;

async function ensureSetup() {
	if (setupDone) return;
	if (setupPromise) return setupPromise;
	setupPromise = doSetup();
	return setupPromise;
}

function ensureWorkerNetwork(): void {
	try {
		execSync(`docker network inspect ${WORKER_NETWORK}`, { stdio: "pipe" });
	} catch {
		execSync(`docker network create ${WORKER_NETWORK}`, { stdio: "pipe" });
	}
}

async function doSetup() {
	ensureWorkerNetwork();

	const FINGERPRINT_PATH = "/tmp/x11web-fingerprint";
	const FRONTEND_DIST_IN_CONTAINER = "/srv/frontend-dist";
	// All host-side ports are pinned per worker so the OIDC URIs
	// can be set on the backend container's env *before* it starts
	// (testcontainers can't update env after start). Fixed offsets
	// per worker keep workers from colliding.
	//
	// Note that there is no separate frontend server in e2e —
	// the backend serves `frontend/dist` at `/` (bind-mounted in
	// below), so `frontendUrl` is the backend's URL.
	const rtcUdpPort = 3003 + Number(WORKER_INDEX);
	backendPort = 3010 + Number(WORKER_INDEX);
	mockOidcPort = 8090 + Number(WORKER_INDEX);
	const mockOidcContainerPort = 8080;

	mockOidcContainer = await new GenericContainer(
		"ghcr.io/navikt/mock-oauth2-server:2.1.10",
	)
		.withNetworkMode(WORKER_NETWORK)
		.withNetworkAliases("mock-oidc")
		.withEnvironment({
			// Tell the mock server to advertise the host-published
			// URL in its discovery doc + ID-token `iss` claim. Both
			// the backend and the browser hit it via this URL, so
			// the issuer string stays consistent across container
			// and host views.
			SERVER_URL: `http://localhost:${mockOidcPort}`,
			LOG_LEVEL: "INFO",
		})
		// `Wait.forHttp` expects 2xx; mock-oauth2-server's root path
		// returns 404 by design. The discovery doc *is* served, so
		// poll that — it also confirms the server is fully up.
		.withWaitStrategy(
			Wait.forHttp(
				"/x11-web/.well-known/openid-configuration",
				mockOidcContainerPort,
			).forStatusCode(200),
		)
		.withExposedPorts({
			container: mockOidcContainerPort,
			host: mockOidcPort,
		})
		.withReuse()
		.start();
	console.log(
		`[worker ${WORKER_INDEX}] Mock OIDC running at http://localhost:${mockOidcPort}`,
	);

	backendContainer = await GenericContainer.fromDockerfile(
		PROJECT_ROOT,
		"Dockerfile.backend",
	)
		.build("x11-web-backend-test", { deleteOnExit: false })
		.then((image) => {
			const built = image
				.withNetworkMode(WORKER_NETWORK)
				.withNetworkAliases("backend")
				.withExposedPorts({ container: 3001, host: backendPort })
				// `localhost` inside the backend container resolves
				// to the host gateway, so the backend hits the
				// host-published mock-oidc port at the *same* URL
				// the browser uses. That keeps the OIDC issuer
				// string identical on both sides — required for the
				// `iss` claim check on the ID token.
				.withExtraHosts([{ host: "localhost", ipAddress: "host-gateway" }])
				// Mount the prebuilt frontend (produced by
				// `global-setup.ts`) into the backend container so
				// the backend's `ServeDir` fallback serves the SPA.
				// Means the browser hits one origin
				// (`localhost:<backendPort>`) for the SPA, the auth
				// routes, and the WS — cookies trivially same-origin.
				.withBindMounts([
					{
						source: path.join(FRONTEND_DIR, "dist"),
						target: FRONTEND_DIST_IN_CONTAINER,
						mode: "ro",
					},
				])
				.withEnvironment({
					// Pin the fingerprint to a fixed path so the harness
					// can `exec cat` it without depending on a $HOME that
					// the backend's slim image doesn't actually set.
					X11WEB_FINGERPRINT_FILE: FINGERPRINT_PATH,
					// Bind the WebRTC UDP socket to a known port inside
					// the container. The same port is published 1:1 to
					// the host below so `127.0.0.1:<port>` reaches it.
					X11WEB_RTC_BIND_ADDR: `0.0.0.0:${rtcUdpPort}`,
					// What the backend tells the browser to dial. The
					// browser runs on the host, so it sees container
					// services through the host loopback.
					X11WEB_RTC_PUBLIC_HOST: "127.0.0.1",
					// Tells the backend to serve the SPA at `/`. The
					// path is the bind-mount target above.
					X11WEB_FRONTEND_DIR: FRONTEND_DIST_IN_CONTAINER,
					// OIDC against the mock provider. `localhost` here
					// resolves through the host-gateway entry above.
					OIDC_ISSUER: `http://localhost:${mockOidcPort}/x11-web`,
					OIDC_CLIENT_ID: "x11-web",
					OIDC_REDIRECT_URI: `http://localhost:${backendPort}/auth/callback`,
					OIDC_POST_LOGIN_REDIRECT: `http://localhost:${backendPort}/`,
				})
				.withWaitStrategy(Wait.forHttp("/health", 3001).forStatusCode(200))
				// Reuse on worker respawn — keyed by image+env+network, all
				// stable per worker — so we re-attach instead of leaking.
				.withReuse();

			// testcontainers-node's `withExposedPorts` only handles TCP.
			// Reach through the protected `hostConfig` to add the UDP
			// port binding ourselves: the WebRTC DataChannel rides UDP,
			// and without an explicit publish the browser on the host
			// can't reach the container's UDP socket.
			const hostConfig = (
				built as unknown as { hostConfig: Record<string, unknown> }
			).hostConfig;
			const createOpts = (
				built as unknown as {
					createOpts: { ExposedPorts?: Record<string, object> };
				}
			).createOpts;
			const portBindings = (hostConfig.PortBindings ?? {}) as Record<
				string,
				Array<{ HostPort: string }>
			>;
			portBindings[`${rtcUdpPort}/udp`] = [{ HostPort: String(rtcUdpPort) }];
			hostConfig.PortBindings = portBindings;
			createOpts.ExposedPorts = {
				...(createOpts.ExposedPorts ?? {}),
				[`${rtcUdpPort}/udp`]: {},
			};

			return built.start();
		});

	backendPort = backendContainer.getMappedPort(3001);
	console.log(
		`[worker ${WORKER_INDEX}] Backend running at localhost:${backendPort} (RTC UDP :${rtcUdpPort})`,
	);

	// Read the QUIC TLS fingerprint the backend wrote at startup.
	// `/health` only returns 200 after the fingerprint hits disk, so
	// no race here.
	const fpResult = await backendContainer.exec(["cat", FINGERPRINT_PATH]);
	if (fpResult.exitCode !== 0) {
		throw new Error(
			`failed to read backend fingerprint (exit ${fpResult.exitCode}): ${fpResult.output}`,
		);
	}
	const fingerprint = fpResult.output.trim();

	sidecarContainer = await GenericContainer.fromDockerfile(
		PROJECT_ROOT,
		"Dockerfile.sidecar",
	)
		.build("x11-web-sidecar-test", { deleteOnExit: false })
		.then((image) =>
			image
				.withNetworkMode(WORKER_NETWORK)
				.withNetworkAliases("sidecar")
				.withHostname("x11web")
				// Privileged mode lets apps that use Linux user-namespaces for
				// sandboxing (browsers, etc.) work the same as on a normal desktop.
				// This is a container environment requirement, not app-specific.
				.withPrivilegedMode()
				.withEnvironment({
					// QUIC + Cap'n Proto wire (replaces the old WS+JSON
					// path). Server-name must match the cert's SAN
					// ("localhost") even when dialing the network alias.
					BACKEND_QUIC_ADDR: "backend:3002",
					BACKEND_SERVER_NAME: "localhost",
					X11WEB_SERVER_FINGERPRINT: fingerprint,
					SIDECAR_NAME: `test-sidecar-${WORKER_INDEX}`,
					DISPLAY_NUMBER: "99",
					// Set DISPLAY too — many tests run subprocess.run([...])
					// inside python that doesn't propagate explicit env, so
					// they pick up the container default.
					DISPLAY: ":99",
					RUST_LOG: process.env.SIDECAR_RUST_LOG ?? "info",
					NO_AT_BRIDGE: "1",
					// Pass through the extension kill switch so a test
					// run can bisect app breakage per X extension, e.g.
					// X11WEB_DISABLE_EXTENSIONS=XInputExtension pnpm
					// exec playwright test …
					X11WEB_DISABLE_EXTENSIONS:
						process.env.X11WEB_DISABLE_EXTENSIONS ?? "",
				})
				// Use a shell-based readiness probe (X socket + backend WS
				// still alive) instead of `Wait.forLogMessage`. Log-based
				// waits search the container's stdout buffer, which on
				// .withReuse() may have already scrolled past the target
				// line — causing 60s timeouts on every worker respawn.
				.withWaitStrategy(
					Wait.forSuccessfulCommand(
						"test -S /tmp/.X11-unix/X99 && pgrep -x x11-web-sidecar >/dev/null",
					),
				)
				.withReuse()
				.start(),
		);

	console.log(
		`[worker ${WORKER_INDEX}] SPA served by backend at http://localhost:${backendPort}`,
	);
	setupDone = true;
}

async function teardownAll() {
	setupDone = false;
	setupPromise = null;
	// Stop containers explicitly so signal-handler teardowns don't leave
	// processes running. global-teardown.ts is the fallback for SIGKILL.
	await Promise.allSettled([
		sidecarContainer?.stop(),
		backendContainer?.stop(),
		mockOidcContainer?.stop(),
	]);
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
		// First-time setup builds two Dockerfiles per worker — always more
		// than the per-test 60s budget. Cached image layers from previous
		// runs make subsequent setups much faster.
		{ scope: "worker", timeout: 600_000 },
	],
	frontendUrl: [
		async ({}, use) => {
			await ensureSetup();
			// SPA + WS + auth all live on the backend's host port —
			// same origin, so cookies trivially apply. The WS URL is
			// still baked in as a `?ws=` query param so the bundle
			// can be shared across workers.
			const wsUrl = `ws://localhost:${backendPort}/ws/frontend`;
			await use(
				`http://localhost:${backendPort}/?ws=${encodeURIComponent(wsUrl)}`,
			);
		},
		{ scope: "worker", timeout: 600_000 },
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
	// Snapshot existing frames by `data-client-id` rather than count.
	// `windows` is keyed off the doc's OcifNode order, not click
	// order; nth(countBefore) can pick a sibling that re-rendered
	// in between (e.g. a parent frame whose z just changed) rather
	// than the brand-new one. Diffing the id set finds the new frame
	// no matter where it lands in the DOM.
	const idsBefore = new Set(
		await windowFrames.evaluateAll((els) =>
			els.map((el) => el.getAttribute("data-client-id") ?? ""),
		),
	);
	const countBefore = idsBefore.size;

	await page.locator('[data-testid="spawn-button"]').click();
	if (command !== "xeyes") {
		await page.locator('input[placeholder="command"]').fill(command);
	}
	if (args) {
		await page.locator('input[placeholder="args"]').fill(args);
	}
	await expect(page.locator("button", { hasText: "Spawn" })).toBeEnabled({
		timeout: 30_000,
	});
	await page.locator("button", { hasText: "Spawn" }).click();

	await expect(windowFrames).toHaveCount(countBefore + 1, {
		timeout: windowTimeout,
	});
	const newId = await page.evaluate((existing: string[]) => {
		const set = new Set(existing);
		const all = Array.from(
			document.querySelectorAll('[data-testid="window-frame"]'),
		);
		const found = all.find(
			(el) => !set.has(el.getAttribute("data-client-id") ?? ""),
		);
		return found?.getAttribute("data-client-id") ?? null;
	}, Array.from(idsBefore));
	if (!newId) {
		throw new Error("spawnApp: failed to identify newly spawned window frame");
	}
	return page.locator(
		`[data-testid="window-frame"][data-client-id="${newId}"]`,
	);
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

/** Fraction of canvas pixels within `tol` of an exact RGB color.
 *  The animation-proof alternative to golden screenshots for "did
 *  input reach the app": a probe page changes to a known color only
 *  in response to a real DOM event, and blinking carets / loading
 *  spinners can't counterfeit a half-screen color flip. */
export async function colorFraction(
	canvas: Locator,
	rgb: [number, number, number],
	tol = 12,
): Promise<number> {
	return canvas.evaluate(
		(el: HTMLCanvasElement, { rgb, tol }) => {
			const ctx = el.getContext("2d");
			if (!ctx || el.width === 0 || el.height === 0) return 0;
			const d = ctx.getImageData(0, 0, el.width, el.height).data;
			let hit = 0;
			const total = el.width * el.height;
			for (let i = 0; i < d.length; i += 4) {
				if (
					Math.abs(d[i] - rgb[0]) <= tol &&
					Math.abs(d[i + 1] - rgb[1]) <= tol &&
					Math.abs(d[i + 2] - rgb[2]) <= tol
				) {
					hit++;
				}
			}
			return hit / total;
		},
		{ rgb, tol },
	);
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
