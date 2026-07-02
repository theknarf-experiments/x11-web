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

	// Per-worker networks (`x11web-worker-*`) created by fixtures.ts.
	const nets = (() => {
		try {
			return execSync(`docker network ls --filter "name=x11web-worker-" -q`, {
				encoding: "utf-8",
				timeout: 10_000,
			}).trim();
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
}
