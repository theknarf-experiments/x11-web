/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";

test.describe("Focus model", () => {
	test("SetInputFocus and GetInputFocus with revert modes", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"setinputfocus_getinputfocus_revert.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/focus-model: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(4);
	});
});
