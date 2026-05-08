import { defineConfig } from "@playwright/test";

// Each worker spawns its own backend + sidecar Docker containers + frontend
// `serve`. Default of 2 workers is a sweet spot: ~40% wall-time win over
// serial without the resource-contention flakes we see at 4+ workers.
// Override with PLAYWRIGHT_WORKERS env on machines with more headroom.
const WORKERS = process.env.PLAYWRIGHT_WORKERS
	? Number(process.env.PLAYWRIGHT_WORKERS)
	: 2;

export default defineConfig({
	testDir: "./tests",
	// 60s is enough for protocol/atom probe tests; specific slow tests
	// (rendercheck full suite, x11perf, etc.) override this with
	// test.setTimeout(...) where they actually need more.
	timeout: 60_000,
	retries: 0,
	workers: WORKERS,
	// Parallelise tests *within* a file across worker slots, not just
	// across files. Each `fixtures.ts` worker has its own container
	// trio, so parallelism gives every test a clean sidecar. Without
	// this, single-file runs serialise through one sidecar and
	// processes / X state leak between tests (xeyes left running,
	// stale focus, atoms accumulated). The previous `workers > 1`
	// behaviour relied on Playwright respawning workers on failures,
	// which accidentally provided that clean-slate effect.
	fullyParallel: true,
	globalSetup: "./global-setup.ts",
	globalTeardown: "./global-teardown.ts",
	use: {
		headless: true,
		screenshot: "only-on-failure",
		trace: "retain-on-failure",
	},
});
