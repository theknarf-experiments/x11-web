/**
 * Test fixtures for the Wayland sidecar suite.
 *
 * Deliberately a *separate* container set from `../fixtures.ts` rather
 * than an extra sidecar bolted onto the shared worker trio. The Dock
 * renders one `[data-testid="spawn-button"]` per connected sidecar
 * (packages/components/src/Dock/Dock.tsx), so a second sidecar on the
 * same backend makes `waitForDock` / `spawnApp` match two elements and
 * fail Playwright's strict mode in *every* existing spec. Standing up
 * our own backend keeps this suite's blast radius at zero — the price
 * is one extra container per worker, and no changes at all to
 * `../fixtures.ts`.
 *
 * Two further simplifications over the X11 fixtures:
 *
 *   - `OIDC_ISSUER` is omitted, so the backend runs in anonymous mode
 *     (crates/auth/src/lib.rs `OidcConfig::from_env`) and no
 *     mock-oauth2 container is needed. Nothing here tests auth.
 *   - Its own port block (backend 3060+idx, RTC UDP 3053+idx) and its
 *     own network name, so it can never collide with the X11 suite's
 *     3010 / 3003 / 8090 blocks when both run in the same session.
 *
 * The pixel and window helpers are imported verbatim from
 * `../fixtures` — they are sidecar-agnostic and duplicating them would
 * be the actual intrusion.
 */

import { execSync } from "node:child_process";
import * as path from "node:path";
import { test as base, expect } from "@playwright/test";
import type { StartedTestContainer } from "testcontainers";
import { GenericContainer, Wait } from "testcontainers";

const PROJECT_ROOT = path.resolve(import.meta.dirname, "../../..");
const FRONTEND_DIR = path.join(PROJECT_ROOT, "frontend");

// Same reasoning as ../fixtures.ts: TEST_PARALLEL_INDEX is the stable
// parallel-slot index, TEST_WORKER_INDEX is not (it increments on
// every worker respawn, leaking a container set per respawn).
const WORKER_INDEX = process.env.TEST_PARALLEL_INDEX ?? "0";
const WORKER_NETWORK = `x11web-wl-worker-${WORKER_INDEX}`;

let backendContainer: StartedTestContainer;
let waylandSidecarContainer: StartedTestContainer;
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
	const rtcUdpPort = 3053 + Number(WORKER_INDEX);
	backendPort = 3060 + Number(WORKER_INDEX);

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
				.withBindMounts([
					{
						source: path.join(FRONTEND_DIR, "dist"),
						target: FRONTEND_DIST_IN_CONTAINER,
						mode: "ro",
					},
				])
				.withEnvironment({
					X11WEB_FINGERPRINT_FILE: FINGERPRINT_PATH,
					X11WEB_RTC_BIND_ADDR: `0.0.0.0:${rtcUdpPort}`,
					X11WEB_RTC_PUBLIC_HOST: "127.0.0.1",
					X11WEB_FRONTEND_DIR: FRONTEND_DIST_IN_CONTAINER,
					// No OIDC_* → anonymous mode. See the module comment.
				})
				.withWaitStrategy(Wait.forHttp("/health", 3001).forStatusCode(200))
				.withReuse();

			// testcontainers-node only publishes TCP; the WebRTC
			// DataChannel rides UDP. Same hostConfig reach-through as
			// ../fixtures.ts.
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
		`[wl worker ${WORKER_INDEX}] Backend running at localhost:${backendPort} (RTC UDP :${rtcUdpPort})`,
	);

	const fpResult = await backendContainer.exec(["cat", FINGERPRINT_PATH]);
	if (fpResult.exitCode !== 0) {
		throw new Error(
			`failed to read backend fingerprint (exit ${fpResult.exitCode}): ${fpResult.output}`,
		);
	}
	const fingerprint = fpResult.output.trim();

	waylandSidecarContainer = await GenericContainer.fromDockerfile(
		PROJECT_ROOT,
		"Dockerfile.sidecar-wayland",
	)
		.build("x11web-sidecar-wayland", { deleteOnExit: false })
		.then((image) =>
			image
				.withNetworkMode(WORKER_NETWORK)
				.withNetworkAliases("sidecar-wayland")
				.withHostname("x11web-wayland")
				.withEnvironment({
					BACKEND_QUIC_ADDR: "backend:3002",
					BACKEND_SERVER_NAME: "localhost",
					X11WEB_SERVER_FINGERPRINT: fingerprint,
					SIDECAR_NAME: `test-wayland-sidecar-${WORKER_INDEX}`,
					WAYLAND_SCREEN_SIZE: "1280x800",
					RUST_LOG: process.env.SIDECAR_RUST_LOG ?? "info",
				})
				// Shell probe rather than Wait.forLogMessage: with
				// .withReuse() the target line may already have scrolled
				// out of the buffer on a worker respawn.
				//
				// `pgrep -f`, NOT `-x`: /proc/<pid>/comm truncates to 15
				// characters and `x11-web-sidecar-wayland` is 23, so an
				// exact-name match can never hit.
				.withWaitStrategy(
					Wait.forSuccessfulCommand(
						"ls /run/user/0/wayland-* >/dev/null 2>&1 && pgrep -f x11-web-sidecar-wayland >/dev/null",
					),
				)
				.withReuse()
				.start(),
		);

	console.log(
		`[wl worker ${WORKER_INDEX}] SPA served by backend at http://localhost:${backendPort}`,
	);
	setupDone = true;
}

async function teardownAll() {
	setupDone = false;
	setupPromise = null;
	await Promise.allSettled([
		waylandSidecarContainer?.stop(),
		backendContainer?.stop(),
	]);
}

export type WaylandFixtures = {
	waylandSidecarContainer: StartedTestContainer;
	frontendUrl: string;
};

export const test = base.extend<{}, WaylandFixtures>({
	waylandSidecarContainer: [
		// Playwright discovers a fixture's dependencies by string-parsing
		// this parameter: `fixtureParameterNames` throws unless the first
		// argument is literally an object destructuring pattern, so a
		// fixture with no dependencies has to spell that as `{}`. (The
		// suppression has to be a single comment line — biome only honours
		// a `biome-ignore` on the line directly above the finding.)
		// biome-ignore lint/correctness/noEmptyPattern: Playwright requires the destructuring pattern
		async ({}, use) => {
			await ensureSetup();
			await use(waylandSidecarContainer);
		},
		// First-time setup builds two Dockerfiles (one of them a cold
		// smithay compile), which is far more than the per-test budget.
		{ scope: "worker", timeout: 900_000 },
	],
	frontendUrl: [
		// biome-ignore lint/correctness/noEmptyPattern: Playwright requires the destructuring pattern
		async ({}, use) => {
			await ensureSetup();
			const wsUrl = `ws://localhost:${backendPort}/ws/frontend`;
			await use(
				`http://localhost:${backendPort}/?ws=${encodeURIComponent(wsUrl)}`,
			);
		},
		{ scope: "worker", timeout: 900_000 },
	],
});

// The pixel/window helpers are sidecar-agnostic — re-export the X11
// suite's rather than growing a second copy that can drift.
export {
	canvasPixelHash,
	colorFraction,
	countNonBlackPixels,
	hasRenderedContent,
	spawnApp,
	waitForCanvasStable,
	waitForDock,
} from "../fixtures";
export { expect };

/** Kill every Wayland client spawned by a test, between tests. */
export async function cleanupWaylandApps(
	container: StartedTestContainer,
): Promise<void> {
	await container
		?.exec([
			"bash",
			"-c",
			"pkill -9 -f 'wl-input-probe|weston-simple-shm|foot|wayland-info' 2>/dev/null; true",
		])
		.catch(() => {});
	await new Promise((r) => setTimeout(r, 1000));
}

for (const signal of ["SIGINT", "SIGTERM"] as const) {
	process.on(signal, () => {
		teardownAll()
			.catch(() => {})
			.finally(() => process.exit());
	});
}
