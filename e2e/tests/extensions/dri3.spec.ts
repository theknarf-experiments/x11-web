/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, waitForDock, runPythonScript } from "../fixtures";

test.describe("DRI3 extension capabilities", () => {
	test.beforeEach(async ({ page, frontendUrl }) => {
		await page.goto(frontendUrl);
		await waitForDock(page);
	});

	// DRI3 was removed from the server (commit 60b4bd3). This test is
	// kept skipped as a placeholder in case DRI3 ever returns.
	test.skip("DRI3 GetSupportedModifiers returns LINEAR modifier", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "dri3_getsupportedmodifiers_linear.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("PASS: DRI3 extension available");
	});
});

test.describe("DRI3 supported modifiers", () => {
	test.skip("DRI3 extension is available", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["xdpyinfo", "-queryExtensions"]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("DRI3");
	});
});
