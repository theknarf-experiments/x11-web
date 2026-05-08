/**
 * E2E compliance tests for Phase 4 spec compliance fixes:
 * - Wide dashed line rendering (line_width > 1 with dash patterns)
 * - VisibilityNotify on geometry changes (not just stacking changes)
 *
 * Per-test python3-xlib scripts live under `e2e/scripts/`; see the
 * `runPythonScript` helper in `fixtures.ts`.
 */

import { test, expect, runPythonScript } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

async function probe(
	container: StartedTestContainer,
	name: string,
): Promise<string> {
	const result = await runPythonScript(container, name, {
		env: { DISPLAY: ":99" },
	});
	return result.output.trim();
}

test.describe.serial("Wide dashed line rendering", () => {
	test.setTimeout(60_000);

	test("wide dashed horizontal line creates visible gaps", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "wide_dashed_horizontal_line.py");
		expect(output).toContain("PASS");
	});

	test("wide dashed vertical line creates visible gaps", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "wide_dashed_vertical_line.py");
		expect(output).toContain("PASS");
	});

	test("DoubleDash wide line draws background in gaps", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "wide_doubledash_line.py");
		expect(output).toContain("PASS");
	});
});

test.describe.serial("VisibilityNotify on geometry changes", () => {
	test.setTimeout(60_000);

	test("VisibilityNotify sent when window is moved to reveal sibling", async ({
		sidecarContainer,
	}) => {
		const output = await probe(
			sidecarContainer,
			"visibilitynotify_on_geometry_move.py",
		);
		expect(output).toContain("PASS");
	});
});
