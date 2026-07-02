/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";

test.describe
	.serial("XEmbed protocol compliance", () => {
		test("_XEMBED and _XEMBED_INFO atoms exist", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xembed_and_xembed_info_atoms_exist.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("xembed_present=True");
			expect(output).toContain("xembed_info_present=True");
		});

		test("System tray atoms are pre-defined", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"system_tray_atoms_are_pre_defined.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("tray_opcode_exists=True");
			expect(output).toContain("tray_s0_exists=True");
		});
	});
