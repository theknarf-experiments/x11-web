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

// Image tags produced by `global-setup.ts`, which is the ONLY place they are
// built. Workers must never build them: see `buildImage` there for why
// (silently-stale images and two workers racing on one tag).
const BACKEND_IMAGE = "x11-web-backend-test";
const SIDECAR_IMAGE = "x11-web-sidecar-test";

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

	// The images are built once by `global-setup.ts`, before any worker
	// exists — see the comment on `buildImage` there. Workers only start them.
	const backendSpec = new GenericContainer(BACKEND_IMAGE)
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
	{
		const hostConfig = (
			backendSpec as unknown as { hostConfig: Record<string, unknown> }
		).hostConfig;
		const createOpts = (
			backendSpec as unknown as {
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
	}

	backendContainer = await backendSpec.start();

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

	sidecarContainer = await new GenericContainer(SIDECAR_IMAGE)
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
			X11WEB_DISABLE_EXTENSIONS: process.env.X11WEB_DISABLE_EXTENSIONS ?? "",
			// GLX is off by default in the server because advertising
			// it makes the first GTK3 client on a display — Firefox
			// included — unable to dispatch input at all. See the
			// comment in `crates/x11-server/src/xserver/mod.rs`.
			// `tests/extensions/glx.spec.ts` is the one suite that
			// needs it, and it is opt-in for exactly this reason.
			X11WEB_ENABLE_GLX: process.env.X11WEB_ENABLE_GLX ?? "",
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
		.start();

	console.log(
		`[worker ${WORKER_INDEX}] SPA served by backend at http://localhost:${backendPort}`,
	);
	// Snapshot the container's infrastructure processes before any test has
	// spawned anything, so the per-test reset can tell "was always here" from
	// "a test started this" without matching on names. See RESET_SCRIPT.
	await captureBaselineProcesses(sidecarContainer);
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

/** Test-scoped fixtures. `x11Clean` is auto — see `cleanupApps` below. */
export type X11TestFixtures = {
	// Playwright types a fixture that provides no value as `void` (that is what
	// `use()` with no argument yields), so this is the idiomatic spelling
	// rather than a confusing one.
	// biome-ignore lint/suspicious/noConfusingVoidType: Playwright's type for a value-less fixture
	x11Clean: void;
};

export const test = base.extend<X11TestFixtures, X11Fixtures>({
	// Reset the worker's shared X server AFTER each test. The fixture is `auto`,
	// so all ~60 spec files get it rather than the 2 that used to opt in.
	//
	// Teardown, not setup, and the ordering matters. Killing an app makes the
	// sidecar emit a `ProcessExited` DELTA; the frontend's dock is built from
	// those deltas and is only re-synced in full when it asks for a
	// `ProcessList` at connect time. Resetting at setup put the kill moments
	// before the next test's `page.goto`, dropping that delta straight into the
	// window where the new page has not subscribed yet — the delta is lost and
	// the dock keeps a phantom entry with nothing to correct it. Observed
	// exactly that: `dbusmenu-test` still in the dock four tests after it died,
	// still owning the menu bar, while its window frame was correctly gone.
	//
	// Running at teardown puts the whole inter-test gap between the kill and
	// the next page load. Playwright runs fixture teardown even when a test
	// times out or fails (confirmed in the traces of the 60s timeouts, which
	// show `Fixture "x11Clean"` under After Hooks), so this is no less
	// crash-safe than resetting at setup.
	x11Clean: [
		async ({ sidecarContainer }, use) => {
			await use();
			await cleanupApps(sidecarContainer);
		},
		{ auto: true },
	],
	sidecarContainer: [
		// Playwright discovers a fixture's dependencies by string-parsing
		// this parameter: `fixtureParameterNames` throws unless the first
		// argument is literally an object destructuring pattern, so a
		// fixture with no dependencies has to spell that as `{}`. (The
		// suppression has to be the line directly above the finding —
		// biome ignores one that is separated by another comment.)
		// biome-ignore lint/correctness/noEmptyPattern: Playwright requires the destructuring pattern
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
		// Playwright discovers a fixture's dependencies by string-parsing
		// this parameter: `fixtureParameterNames` throws unless the first
		// argument is literally an object destructuring pattern, so a
		// fixture with no dependencies has to spell that as `{}`. (The
		// suppression has to be the line directly above the finding —
		// biome ignores one that is separated by another comment.)
		// biome-ignore lint/correctness/noEmptyPattern: Playwright requires the destructuring pattern
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

	// Wait for an id that was NOT in the snapshot, rather than for the frame
	// count to reach `idsBefore.size + 1`.
	//
	// The count was the wrong condition and it failed one test in each of
	// three consecutive full runs — a different test each time (`:135`, `:79`,
	// `:186`) but always the same call log: "N x locator resolved to 2
	// elements ... M x resolved to 1 element", expected 3, received 1. The
	// window list is backend-authoritative and arrives asynchronously after
	// `page.goto`, so the snapshot can be taken while rows from the previous
	// test are still draining out of it. Once two of them leave, no later
	// state can ever equal `2 + 1`, and the spawn that actually succeeded
	// times out at a count it can never reach.
	//
	// Waiting on the id is also strictly STRONGER than the count, not a
	// relaxation: `count === before + 1` is satisfiable by one removal plus
	// two additions, whereas "an id we have not seen before is present" is
	// exactly the thing the caller asked for, and it is what the code below
	// already used to pick the frame to return.
	const newIdHandle = await page.waitForFunction(
		(existing: string[]) => {
			const set = new Set(existing);
			const found = Array.from(
				document.querySelectorAll('[data-testid="window-frame"]'),
			).find((el) => !set.has(el.getAttribute("data-client-id") ?? ""));
			return found?.getAttribute("data-client-id") ?? null;
		},
		Array.from(idsBefore),
		{ timeout: windowTimeout },
	);
	const newId = await newIdHandle.jsonValue();
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

// ---------------------------------------------------------------------------
// Per-test X server reset
// ---------------------------------------------------------------------------
//
// `sidecarContainer` is worker-scoped, so ONE X server serves every test that
// worker ever runs (~160 tests per server at 2 workers). Nothing about that
// server is per-test: its clients, its pointer and its selections all persist.
// So state a test leaves behind is state the next test on that worker sees.
//
// The previous cleanup was an opt-in ALLOWLIST of app names, wired into only 2
// of ~60 spec files. It had three defects, all fixed here:
//
//   1. It missed apps. `xcalc`, `gtkprobe` and `xev` are spawned by the suite
//      and were absent from the pattern, so they stayed mapped for the rest of
//      the run and showed up as extra `[data-testid="window-frame"]`s in later
//      tests (the frontend's WindowList is backend-authoritative, so a leaked
//      X client is a visible window on the next page load). Any app added to
//      the suite in future would have silently joined them.
//   2. `pkill -9 -f '<pattern>'` matched the `bash -c` shell running it,
//      because that shell's own command line contains the pattern. It
//      SIGKILLed itself, so the exec's exit status was meaningless and both
//      call sites swallowed it with `.catch(() => {})` — cleanup could
//      half-fail invisibly. Killing by pid avoids pattern matching entirely.
//   3. It reset no other shared X state (pointer, selections), and it padded
//      with a blanket 2s sleep — a guess, not a condition, and ~6.7 minutes of
//      pure sleep across this suite.
//
// Instead, identify the survivors POSITIONALLY rather than by name. Right
// after the sidecar container is ready — before any test has run — snapshot
// the processes that exist (`captureBaselineProcesses`). Those are exactly the
// container's infrastructure: the sidecar itself, the `pulseaudio` from
// crates/sidecar/entrypoint.sh, and the `dbus-daemon` the sidecar starts for
// AppMenu export. Everything that exists later and is not in that snapshot was
// started by a test, so the reset kills it.
//
// This is name-free in both directions, which matters more than it sounds:
//
//   * It kills whole process TREES, not just the sidecar's direct children.
//     Measured: `firefox-esr` forks nine grandchildren (Web Content, RDD
//     Process, Socket Process, ...) that a direct-children-only kill orphans
//     rather than reaps.
//   * It does NOT protect every process merely called `dbus-daemon`. Measured:
//     firefox starts its own `dbus-launch` + `dbus-daemon` pair as siblings of
//     the sidecar's, so an exclusion keyed on the name would spare one stray
//     session bus per firefox test, forever.
//
// Two guards on the kill list: processes with ppid 0 are `docker exec`
// sessions (verified — that is how the container reports them), including the
// shell running this very script, so they must survive; and a baseline pid is
// only honoured if its `comm` still matches what was recorded, so pid reuse
// over a long run cannot silently protect an app.
const RESET_SCRIPT = `
set -u
export DISPLAY=:99
# BASELINE_PIDS is substituted per worker: "pid:comm pid:comm ..."
baseline="__BASELINE__"
victims=$(ps -eo pid=,ppid=,stat=,comm= | awk -v base="$baseline" '
  BEGIN { n = split(base, a, " "); for (i = 1; i <= n; i++) { split(a[i], kv, ":"); keep[kv[1]] = kv[2] } }
  $2 == 0 { next }                      # docker exec sessions, incl. this one
  $1 == 1 { next }                      # container init (the sidecar)
  $3 ~ /^Z/ { next }                    # already dead, awaiting reap
  ($1 in keep) && keep[$1] == $4 { next }  # infrastructure from the snapshot
  { print $1 }
')
killed=""
if [ -n "$victims" ]; then
  kill -9 $victims 2>/dev/null
  killed=yes
fi
# Wait for the X server to actually drop those client connections.
for _ in $(seq 1 50); do
  n=$(xlsclients 2>/dev/null | wc -l)
  [ "$n" -eq 0 ] && break
  sleep 0.1
done
# xlsclients reaching 0 only proves the X SERVER has dropped the clients. The
# frontend's window list is backend-authoritative and travels
# sidecar -> backend -> browser, and that hop is not observable from in here.
# Without this settle the next test can navigate while dead windows are still
# in the list and then watch them drain away mid-test: measured directly, a
# spawnApp snapshotted countBefore=2 and the failure DOM had 0 frames.
# Only pay it when we actually killed something.
[ -n "$killed" ] && sleep 1
# Reset the remaining server-wide state a later test could otherwise inherit:
# the pointer (shared, initialised once per sidecar and never reset) and the
# X selections (the clipboard round-trip test asserts on their contents).
xdotool mousemove 0 0 2>/dev/null
xsel -c -b 2>/dev/null
xsel -c -p 2>/dev/null
rm -rf /root/.mozilla /root/.cache/mozilla 2>/dev/null
true
`;

/** `pid:comm` pairs for the container's infrastructure processes, snapshotted
 *  once per worker before any test runs. Empty until `doSetup` fills it. */
let baselineProcesses = "";

// Cache the snapshot in the container, not just in this process. Playwright
// respawns a worker after a failure and `ensureSetup()` then re-attaches to the
// SAME running container via `.withReuse()` — at which point tests have already
// spawned apps, and a fresh `ps` would enrol those apps as "infrastructure" and
// protect them for the rest of the run. Writing the snapshot on first sight and
// reading it back afterwards keeps it pinned to the container's clean state.
const BASELINE_FILE = "/tmp/x11web-baseline-processes";
const CAPTURE_BASELINE = `
if [ ! -s ${BASELINE_FILE} ]; then
  ps -eo pid=,ppid=,comm= | awk '$2 == 1 { print $1 ":" $3 }' | tr '\\n' ' ' > ${BASELINE_FILE}
fi
cat ${BASELINE_FILE}
`;

async function captureBaselineProcesses(
	container: StartedTestContainer,
): Promise<void> {
	try {
		const r = await container.exec(["bash", "-c", CAPTURE_BASELINE]);
		baselineProcesses = r.output.trim().replace(/\s+/g, " ");
		console.log(
			`[worker ${WORKER_INDEX}] baseline processes: ${baselineProcesses}`,
		);
	} catch {
		// Leave it empty: the reset then spares only pid 1 and the exec
		// sessions, which is over-aggressive but still safe (the sidecar
		// restarts dbus-daemon lazily) and never silently under-cleans.
		baselineProcesses = "";
	}
}

/**
 * Reset the worker's X server to a clean slate: no client apps, pointer at the
 * origin, empty selections. Runs automatically before every test via the
 * `x11Clean` auto-fixture; exported for the few places that need an extra
 * mid-test reset.
 *
 * Bounded: a `docker exec` that hangs under load must not eat the test's whole
 * budget, so give up after 20s and let the test proceed rather than block.
 *
 * A reset that does not complete is REPORTED, never swallowed. Silently
 * half-failing cleanup is exactly what made the old allowlist version so hard
 * to reason about: it SIGKILLed its own shell, both call sites discarded the
 * error, and the only symptom was another test failing later on a stale window
 * count. If a run shows leaked frames, this log line is the thing to look for.
 */
export async function cleanupApps(
	container: StartedTestContainer,
): Promise<void> {
	if (!container) return;
	const script = RESET_SCRIPT.replace("__BASELINE__", baselineProcesses);
	const TIMED_OUT = Symbol("reset-timeout");
	const outcome = await Promise.race([
		container
			.exec(["bash", "-c", script])
			.then((r) =>
				r.exitCode === 0 ? null : `exit ${r.exitCode}: ${r.output}`,
			)
			.catch((e) => `threw: ${e}`),
		new Promise<typeof TIMED_OUT>((r) =>
			setTimeout(() => r(TIMED_OUT), 20_000),
		),
	]);
	if (outcome === TIMED_OUT) {
		console.warn(
			`[worker ${WORKER_INDEX}] X reset did not finish within 20s — the next test may see leaked windows`,
		);
	} else if (outcome) {
		console.warn(`[worker ${WORKER_INDEX}] X reset failed: ${outcome}`);
	}
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
