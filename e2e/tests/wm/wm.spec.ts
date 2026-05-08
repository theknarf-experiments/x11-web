/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";

test.describe.serial("Window manager compliance", () => {
	test.setTimeout(60_000);

	test("override-redirect windows bypass SubstructureRedirect", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "override_redirect_windows_bypass_substructureredirect.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("OVERRIDE_REDIRECT_OK");
	});

	test("window stacking operations (raise/lower)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "window_stacking_operations_raise_lower.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("STACKING_OK");
	});

	test("focus model (Passive, Locally Active) works", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "focus_model_passive_locally_active_works.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("FOCUS_SET_OK");
		expect(output).toContain("FOCUS_POINTERROOT_OK");
	});
});
