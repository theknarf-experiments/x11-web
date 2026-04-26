import { defineConfig } from "@playwright/test";

export default defineConfig({
	testDir: "./tests",
	// 60s is enough for protocol/atom probe tests; specific slow tests
	// (rendercheck full suite, x11perf, etc.) override this with
	// test.setTimeout(...) where they actually need more.
	timeout: 60_000,
	retries: 0,
	workers: 1,
	globalTeardown: "./global-teardown.ts",
	use: {
		headless: true,
		screenshot: "only-on-failure",
		trace: "retain-on-failure",
	},
});
