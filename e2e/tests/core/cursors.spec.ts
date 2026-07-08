/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import type { StartedTestContainer } from "testcontainers";
import { expect, runPythonScript, test } from "../fixtures";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	_timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; ${cmd}`,
	]);
	return result.output.trim();
}

test.describe
	.serial("Font system deep tests", () => {
		test("xlsfonts lists available fonts", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				"xlsfonts 2>&1 | head -50",
			);
			expect(output).not.toContain("unable to open display");
			// Should list some fonts
			expect(output.split("\n").length).toBeGreaterThan(3);
		});

		test("xlsfonts XLFD pattern matching works", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				'xlsfonts -fn "-*-*-*-*-*-*-13-*-*-*-*-*-*-*" 2>&1 | head -20',
			);
			// Should match fonts with pixel size 13
			expect(output).not.toContain("unable to open display");
		});

		test("xlsfonts finds fixed font", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				'xlsfonts -fn "fixed" 2>&1',
			);
			expect(output).toContain("fixed");
		});

		test("xlsfonts finds cursor font", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				'xlsfonts -fn "cursor" 2>&1',
			);
			expect(output).toContain("cursor");
		});

		test("OpenFont and QueryFont round-trip for XLFD pattern", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"openfont_and_queryfont_round_trip_for_xlfd_pattern.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("FONT_OK");
			expect(output).toMatch(/font_ascent=\d+/);
		});

		test("QueryTextExtents returns correct metrics", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"querytextextents_returns_correct_metrics.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("EXTENTS_OK");
		});
	});
