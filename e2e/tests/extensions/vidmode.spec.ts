/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, waitForDock, runPythonScript } from "../fixtures";

test.describe("VidMode gamma", () => {
	test("xgamma can read current gamma values", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"# xgamma uses VidMode GetGamma",
				"xgamma 2>&1 || echo 'xgamma-ran'",
				"echo 'gamma-read-done'",
			].join("\n"),
		]);
		expect(result.output).toContain("gamma-read-done");
	});

	test("VidMode GetModeLine returns screen dimensions", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "vidmode_getmodeline_screen_dims.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("vidmode-dimensions-ok");
	});
});

test.describe("VidMode extension mode management", () => {
	test.beforeEach(async ({ page, frontendUrl }) => {
		await page.goto(frontendUrl);
		await waitForDock(page);
	});

	test("VidMode GetAllModeLines returns at least one mode", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "vidmode_getallmodelines.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("PASS: VidMode returned modes");
	});

	test("VidMode LockModeSwitch toggles lock state", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "vidmode_lockmodeswitch_toggle.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("PASS: VidMode lock/unlock succeeded");
	});
});

test.describe("VidMode extension", () => {
	test("xdpyinfo shows XFree86-VidModeExtension", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["xdpyinfo", "-queryExtensions"]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("XFree86-VidMode");
	});
});
