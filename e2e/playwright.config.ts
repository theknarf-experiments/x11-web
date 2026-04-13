import { defineConfig } from "@playwright/test";

export default defineConfig({
	testDir: "./tests",
	timeout: 300_000,
	retries: 0,
	use: {
		headless: true,
		screenshot: "only-on-failure",
		trace: "retain-on-failure",
	},
});
