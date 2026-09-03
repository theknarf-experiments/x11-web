/**
 * Playwright global teardown — kills any Docker containers and serve
 * processes left behind by test workers.  This runs once after ALL
 * workers have exited, so it catches anything that per-worker cleanup
 * missed (e.g. because the worker was SIGKILLed).
 */

import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

function run(cmd: string) {
	try {
		execSync(cmd, { stdio: "pipe", timeout: 30_000 });
	} catch {
		// ignore — container/process may already be gone
	}
}

export default function globalTeardown() {
	// Drop per-worker frontend lockfiles created by fixtures.ts.
	for (const f of fs.readdirSync(os.tmpdir())) {
		if (/^x11web-worker-\d+\.json$/.test(f)) {
			try {
				fs.unlinkSync(path.join(os.tmpdir(), f));
			} catch {
				/* not present */
			}
		}
	}

	// Stop all containers created by testcontainers (includes per-worker
	// backend, sidecar, and Ryuk).  The label filter is reliable regardless
	// of whether the image was tagged or referenced by hash.
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

	// Kill any orphaned `serve` processes spawned from e2e (one per worker).
	run("pkill -f 'serve dist -l .* --no-clipboard' || true");

	// Drop the per-worker Docker networks created by the fixtures
	// modules: `x11web-worker-*` (tests/fixtures.ts) and
	// `x11web-wl-worker-*` (tests/wayland/fixtures.ts). Both prefixes
	// are spelled out: `--filter name=` is a substring match, so the
	// shorter `x11web-` would also sweep up (and delete) any network a
	// developer created with that string anywhere in its name. Repeated
	// `name=` filters are OR-ed by docker.
	const networks = (() => {
		try {
			return execSync(
				`docker network ls --filter "name=x11web-worker-" --filter "name=x11web-wl-worker-" -q`,
				{ encoding: "utf-8", timeout: 10_000 },
			).trim();
		} catch {
			return "";
		}
	})();
	if (networks) {
		for (const id of networks.split("\n").filter(Boolean)) {
			run(`docker network rm ${id}`);
		}
	}

	// Prune any other orphaned Docker networks created by testcontainers
	run("docker network prune -f");
}
