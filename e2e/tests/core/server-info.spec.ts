/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { expect, test } from "../fixtures";

test.describe("Conformance: X-Resource extension", () => {
	test("xdpyinfo lists X-Resource extension", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xdpyinfo -queryExtensions 2>&1 | grep -i 'X-Resource'",
		]);
		expect(result.output).toContain("X-Resource");
	});
});
