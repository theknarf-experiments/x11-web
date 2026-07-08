/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { expect, runPythonScript, test } from "../fixtures";

test.describe("DAMAGE extension", () => {
	test("DamageCreate and DamageDestroy work without errors", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"damage_create_destroy.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/damage-basic: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});
});
