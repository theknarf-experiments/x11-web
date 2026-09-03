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
	// The Wayland suite builds its own sidecar image (a cold smithay
	// release compile, plus a weston .deb extraction and a
	// wayland-scanner probe build) and stands up its own backend on top
	// of the X11 trio — well past what the default run should pay, and a
	// build timeout there would fail the whole suite rather than one
	// spec. So it is opt-in:
	//
	//     X11WEB_WAYLAND_E2E=1 pnpm exec playwright test tests/wayland
	//
	// The env var is the ONLY way in. Naming the path on the command
	// line does not help: positional CLI arguments filter files that
	// have already been collected, and `testIgnore` removes them before
	// collection — verified, `playwright test tests/wayland --list`
	// reports "Total: 0 tests in 0 files" without the variable.
	testIgnore: process.env.X11WEB_WAYLAND_E2E ? [] : ["**/tests/wayland/**"],
	// 60s is enough for protocol/atom probe tests; specific slow tests
	// (rendercheck full suite, x11perf, etc.) override this with
	// test.setTimeout(...) where they actually need more.
	timeout: 60_000,
	retries: 0,
	workers: WORKERS,
	// Parallelise tests *within* a file across worker slots, not just
	// across files, so a single 200-test file still uses both slots.
	//
	// This does NOT hand each test a clean sidecar — an earlier version
	// of this comment claimed it did, and that misconception is what
	// made the suite's flakiness so hard to read. `sidecarContainer` is
	// worker-scoped, so ONE X server serves every test a slot ever runs
	// (~160 of them here). Worker respawn does not reset it either:
	// `ensureSetup()` re-enters `.withReuse()` and re-attaches to the
	// same running container, X clients, pointer, focus and all.
	//
	// What actually gives each test a clean slate is the auto
	// `x11Clean` fixture in `fixtures.ts`, which resets the worker's X
	// server at teardown. Parallelism only randomises the order tests
	// land in; it never cleaned anything up.
	fullyParallel: true,
	globalSetup: "./global-setup.ts",
	globalTeardown: "./global-teardown.ts",
	use: {
		headless: true,
		screenshot: "only-on-failure",
		trace: "retain-on-failure",
	},
});
