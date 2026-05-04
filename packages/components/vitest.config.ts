import path from "node:path";
import { fileURLToPath } from "node:url";
import { storybookTest } from "@storybook/addon-vitest/vitest-plugin";
import { playwright } from "@vitest/browser-playwright";
import { defineConfig } from "vitest/config";

const dirname = path.dirname(fileURLToPath(import.meta.url));

// Run every story as a Vitest test in a real Chromium instance via
// Playwright. EditContext only ships in Chromium-family browsers,
// so headless Chrome is the only realistic place to verify the
// component end-to-end.
export default defineConfig({
	test: {
		projects: [
			{
				extends: true,
				plugins: [
					storybookTest({
						configDir: path.join(dirname, ".storybook"),
					}),
				],
				test: {
					name: "storybook",
					browser: {
						enabled: true,
						provider: playwright(),
						headless: true,
						instances: [{ browser: "chromium" }],
					},
					setupFiles: [
						path.join(dirname, ".storybook/vitest.setup.ts"),
					],
				},
			},
		],
	},
});
