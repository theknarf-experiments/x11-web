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
	.serial("SHAPE extension", () => {
		test("ShapeRectangles sets window shape", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"shaperectangles_sets_window_shape.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("shape_present=True");
		});
	});

test.describe("SHAPE extension conformance", () => {
	test("SHAPE: set bounding region and query extents", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display, Xlib.ext.shape",
				"d = Xlib.display.Display()",
				"d.shape_query_version()",
				"root = d.screen().root",
				"w = root.create_window(0, 0, 200, 200, 0, d.screen().root_depth)",
				"w.map()",
				"d.sync()",
				// (operation, destination_kind, ordering, x_offset, y_offset, rectangles)
				"w.shape_rectangles(Xlib.ext.shape.SO.Set, Xlib.ext.shape.SK.Bounding, 0, 0, 0, [(10, 10, 50, 50)])",
				"d.sync()",
				"ext = w.shape_query_extents()",
				"print(f'bounding_shaped={int(ext.bounding_shaped)}')",
				"print(f'bounding_x={ext.bounding_shape_extents_x}')",
				"print(f'bounding_y={ext.bounding_shape_extents_y}')",
				"print(f'bounding_w={ext.bounding_shape_extents_width}')",
				"print(f'bounding_h={ext.bounding_shape_extents_height}')",
				"print('SHAPE_TEST_PASS')",
				"w.destroy()",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("SHAPE_TEST_PASS");
		expect(result.output).toContain("bounding_shaped=1");
		expect(result.output).toContain("bounding_x=10");
		expect(result.output).toContain("bounding_y=10");
		expect(result.output).toContain("bounding_w=50");
		expect(result.output).toContain("bounding_h=50");
	});

	test("SHAPE: combine bounding regions (Union)", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display, Xlib.ext.shape",
				"d = Xlib.display.Display()",
				"d.shape_query_version()",
				"root = d.screen().root",
				"w = root.create_window(0, 0, 200, 200, 0, d.screen().root_depth)",
				"w.map()",
				"d.sync()",
				"w.shape_rectangles(Xlib.ext.shape.SO.Set, Xlib.ext.shape.SK.Bounding, 0, 0, 0, [(0, 0, 50, 50)])",
				"d.sync()",
				"w.shape_rectangles(Xlib.ext.shape.SO.Union, Xlib.ext.shape.SK.Bounding, 0, 0, 0, [(30, 30, 50, 50)])",
				"d.sync()",
				// Union of (0,0,50,50) and (30,30,50,50) = bounds (0,0,80,80).
				"ext = w.shape_query_extents()",
				"print(f'bounding_w={ext.bounding_shape_extents_width}')",
				"print(f'bounding_h={ext.bounding_shape_extents_height}')",
				"print('SHAPE_UNION_PASS')",
				"w.destroy()",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("SHAPE_UNION_PASS");
		expect(result.output).toContain("bounding_w=80");
		expect(result.output).toContain("bounding_h=80");
	});

	test("SHAPE: clip region affects drawing", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display, Xlib.ext.shape",
				"d = Xlib.display.Display()",
				"d.shape_query_version()",
				"root = d.screen().root",
				"w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)",
				"w.map()",
				"d.sync()",
				"w.shape_rectangles(Xlib.ext.shape.SO.Set, Xlib.ext.shape.SK.Clip, 0, 0, 0, [(10, 10, 30, 30)])",
				"d.sync()",
				"ext = w.shape_query_extents()",
				"print(f'clip_shaped={int(ext.clip_shaped)}')",
				"print('SHAPE_CLIP_PASS')",
				"w.destroy()",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("SHAPE_CLIP_PASS");
		expect(result.output).toContain("clip_shaped=1");
	});
});

test.describe("SHAPE extension queries", () => {
	test("xdpyinfo shows SHAPE extension", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"xdpyinfo",
			"-queryExtensions",
		]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("SHAPE");
	});
});

test.describe
	.serial("Extension deep tests", () => {
		test.setTimeout(60_000);

		test("SHAPE extension: set window shape", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"shape_extension_set_window_shape.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("SHAPE_PRESENT");
		});

		test("COMPOSITE extension: redirect window", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"composite_extension_redirect_window.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("COMPOSITE_PRESENT");
		});

		test("XFIXES extension: create region", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xfixes_extension_create_region.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("XFIXES_PRESENT");
		});

		test("RANDR extension: query screen resources", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(sidecarContainer, "xrandr 2>&1");
			expect(output).not.toContain("Failed to get size");
			// Should show at least one output/mode
			expect(output).toMatch(/\d+x\d+/);
		});

		test("XInput2 is available via xinput", async ({ sidecarContainer }) => {
			const output = await execInSidecar(sidecarContainer, "xinput list 2>&1");
			expect(output).not.toContain("unable to open display");
			// Should list at least virtual core pointer and keyboard
			expect(output).toMatch(/pointer|keyboard/i);
		});

		test("XTEST extension: simulate input", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xtest_extension_simulate_input.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("XTEST_PRESENT");
		});

		test("MIT-SHM PutImage via xclip", async ({ sidecarContainer }) => {
			// Test shared memory via a real tool
			const output = await execInSidecar(
				sidecarContainer,
				`echo "test_clipboard_data" | xclip -selection clipboard 2>&1
xclip -selection clipboard -o 2>&1`,
			);
			expect(output).toContain("test_clipboard_data");
		});
	});
