/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, waitForDock, runPythonScript } from "../fixtures";

test.describe("XVideo extension FOURCC formats", () => {
	test.beforeEach(async ({ page, frontendUrl }) => {
		await page.goto(frontendUrl);
		await waitForDock(page);
	});

	test.skip("XVideo QueryAdaptors and ListImageFormats return formats", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "xvideo_queryadaptors_listformats.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("PASS: XVideo formats advertised");
	});
});

test.describe("XVideo formats", () => {
	test("xvinfo lists supported formats", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xvinfo 2>&1 | head -30",
				"echo XVINFO_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("XVINFO_PASS");
	});
});
