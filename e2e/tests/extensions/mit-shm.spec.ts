/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect } from "../fixtures";

test.describe("SHM extension", () => {
	test("MIT-SHM extension is available", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"xdpyinfo",
			"-queryExtensions",
		]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("MIT-SHM");
	});
});
