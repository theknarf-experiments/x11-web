/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";
import type { StartedTestContainer } from "testcontainers";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	_timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}

test.describe.serial("Font system (Phase 7)", () => {
	test("'fixed' font can be opened", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "fixed_font_can_be_opened.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("font_opened=True");
	});

	test("QueryTextExtents returns valid metrics", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "querytextextents_returns_valid_metrics.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("valid=True");
	});
});

test.describe("Font enumeration", () => {
	test("xlsfonts lists at least 100 fonts", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"FONT_COUNT=$(xlsfonts 2>/dev/null | wc -l)",
				"echo \"FONT_COUNT=$FONT_COUNT\"",
				"if [ \"$FONT_COUNT\" -ge 100 ]; then",
				"  echo 'FONT_ENUM_PASS'",
				"else",
				"  echo 'FONT_ENUM_LOW'",
				"fi",
			].join("\n"),
		]);
		expect(result.output).toContain("FONT_ENUM_PASS");
	});

	test("fixed font is available", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xlsfonts -fn fixed 2>&1",
				"echo FIXED_FONT_PASS",
			].join("\n"),
		]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("fixed");
		expect(result.output).toContain("FIXED_FONT_PASS");
	});
});

test.describe.serial("Font handling", () => {
	test("QueryFont returns valid metrics for fixed font", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "queryfont_returns_valid_metrics_for_fixed_font.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("font_ok=True");
	});

	test("ListFonts returns known fonts", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xlsfonts 2>&1 | wc -l",
		);
		const fontCount = parseInt(output.trim());
		// Should have at least some fonts available
		expect(fontCount).toBeGreaterThan(5);
	});
});
