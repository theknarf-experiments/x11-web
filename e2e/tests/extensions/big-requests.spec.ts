/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, waitForDock } from "../fixtures";

test.describe("Big requests extension", () => {
	test.beforeEach(async ({ page, frontendUrl }) => {
		await page.goto(frontendUrl);
		await waitForDock(page);
	});

	test("BIG-REQUESTS extension is available", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"set -e",
				"export DISPLAY=:99",
				"DISPLAY=:99 xdpyinfo | grep -i 'BIG-REQUESTS' && echo 'PASS: BIG-REQUESTS listed' || echo 'FAIL: BIG-REQUESTS not found'",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS: BIG-REQUESTS listed");
	});
});
