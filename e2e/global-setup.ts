/**
 * Playwright global setup — runs once before any worker starts.
 *
 * Builds the frontend bundle so every worker can spawn its own `serve`
 * process against the shared `frontend/dist` output (no rebuild races).
 * The runtime WS URL is supplied per-page via a `?ws=` query param so the
 * single bundle works regardless of which worker / backend port a test
 * happens to land on.
 *
 * Also wipes any orphaned containers + networks left behind by a
 * previous run that was SIGKILL'd before global-teardown could run.
 * Without this they accumulate forever — `.withReuse()` exempts them
 * from Ryuk's reaping, and SIGKILL skips globalTeardown.
 */

import { exec, execSync } from "node:child_process";
import * as path from "node:path";

const PROJECT_ROOT = path.resolve(import.meta.dirname, "..");
const FRONTEND_DIR = path.join(PROJECT_ROOT, "frontend");

function silentSync(cmd: string) {
	try {
		execSync(cmd, { stdio: "pipe", timeout: 30_000 });
	} catch {
		// ignore — already gone or docker not available
	}
}

function reapOrphans() {
	// Containers from prior runs (testcontainers labels everything it
	// creates with `org.testcontainers=true`).
	const ids = (() => {
		try {
			return execSync(
				'docker ps -aq --filter "label=org.testcontainers=true"',
				{ encoding: "utf-8", timeout: 10_000 },
			).trim();
		} catch {
			return "";
		}
	})();
	if (ids) {
		for (const id of ids.split("\n").filter(Boolean)) {
			silentSync(`docker rm -f ${id}`);
		}
	}

	// Per-worker networks created by the fixtures modules:
	// `x11web-worker-*` (tests/fixtures.ts) and `x11web-wl-worker-*`
	// (tests/wayland/fixtures.ts). Both prefixes are spelled out rather
	// than shortened to `x11web-`: `--filter name=` is a *substring*
	// match, so the short form would also match — and then `docker
	// network rm` — any network a developer happens to have named with
	// `x11web-` anywhere in it. Repeated `name=` filters are OR-ed.
	const nets = (() => {
		try {
			return execSync(
				`docker network ls --filter "name=x11web-worker-" --filter "name=x11web-wl-worker-" -q`,
				{ encoding: "utf-8", timeout: 10_000 },
			).trim();
		} catch {
			return "";
		}
	})();
	if (nets) {
		for (const id of nets.split("\n").filter(Boolean)) {
			silentSync(`docker network rm ${id}`);
		}
	}
}

/**
 * Build one of the test images with the docker CLI, once, before any worker
 * exists.
 *
 * This used to live in the worker fixtures as
 * `GenericContainer.fromDockerfile(PROJECT_ROOT, ...).build(tag)`, which was
 * wrong in two ways that both cost real debugging time:
 *
 *   1. It did not reliably rebuild. Measured: after editing
 *      `crates/x11-server/src/menus.rs` a full fixture-driven run still ran
 *      the *previous* binary — `docker history x11-web-sidecar-test` showed
 *      the `COPY .../x11-web-sidecar` layer two hours old, and a subsequent
 *      `docker build` of the same context took 1m41s recompiling the crate.
 *      A harness that silently tests a stale binary makes every measurement
 *      taken through it worthless, which is far worse than a slow one.
 *   2. Both workers raced to build the SAME tag from the same context at the
 *      same moment, inside the fixture's 600s budget. On a cold cache that is
 *      two full parallel Rust builds competing for the whole machine; if
 *      either overran, the worker fixture failed and every test queued behind
 *      it reported "did not run".
 *
 * Building here fixes both: one build, before the first worker starts, with
 * the same builder a developer gets from the command line.
 */
function buildImage(tag: string, dockerfile: string) {
	const started = Date.now();
	try {
		execSync(`docker build -f ${dockerfile} -t ${tag} .`, {
			cwd: PROJECT_ROOT,
			stdio: "pipe",
			// A cold Rust release build of the sidecar is minutes, not
			// seconds; the frontend build above is the only other thing
			// on the machine at this point.
			timeout: 1_800_000,
		});
	} catch (e) {
		const err = e as { stderr?: Buffer; stdout?: Buffer };
		console.error(
			`docker build ${dockerfile} failed:\n${err.stderr?.toString() ?? ""}${err.stdout?.toString() ?? ""}`,
		);
		throw e;
	}
	console.log(
		`[global-setup] built ${tag} in ${((Date.now() - started) / 1000).toFixed(1)}s`,
	);
}

export default async function globalSetup() {
	reapOrphans();

	await new Promise<void>((resolve, reject) => {
		exec("pnpm run build", { cwd: FRONTEND_DIR }, (error, _stdout, stderr) => {
			if (error) {
				console.error("Frontend build failed:", stderr);
				reject(error);
			} else {
				resolve();
			}
		});
	});

	buildImage("x11-web-backend-test", "Dockerfile.backend");
	buildImage("x11-web-sidecar-test", "Dockerfile.sidecar");
}
