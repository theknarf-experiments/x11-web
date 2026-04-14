/**
 * Playwright global teardown — kills any Docker containers and serve
 * processes left behind by test workers.  This runs once after ALL
 * workers have exited, so it catches anything that per-worker cleanup
 * missed (e.g. because the worker was SIGKILLed).
 */

import { execSync } from "node:child_process";

function run(cmd: string) {
	try {
		execSync(cmd, { stdio: "pipe", timeout: 30_000 });
	} catch {
		// ignore — container/process may already be gone
	}
}

export default function globalTeardown() {
	// Stop all containers created by testcontainers (includes backend,
	// sidecar, and Ryuk).  The label filter is reliable regardless of
	// whether the image was tagged or referenced by hash.
	const containerIds = (() => {
		try {
			return execSync(
				'docker ps -aq --filter "label=org.testcontainers=true"',
				{ encoding: "utf-8", timeout: 10_000 },
			).trim();
		} catch {
			return "";
		}
	})();

	if (containerIds) {
		for (const id of containerIds.split("\n").filter(Boolean)) {
			run(`docker rm -f ${id}`);
		}
	}

	// Kill any orphaned `serve` processes spawned from e2e
	run("pkill -f 'serve dist -l .* --no-clipboard' || true");

	// Prune orphaned Docker networks created by testcontainers
	run("docker network prune -f");
}
