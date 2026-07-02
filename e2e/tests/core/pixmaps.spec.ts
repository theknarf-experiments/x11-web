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
	const result = await container.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; ${cmd}`,
	]);
	return result.output.trim();
}

test.describe
	.serial("Drawing operations compliance", () => {
		test("PolyFillRectangle with GC function XOR", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"polyfillrectangle_with_gc_function_xor.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("xor_correct=True");
		});

		test("CopyPlane between depths", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(sidecarContainer, "copyplane_between_depths.py", {
					env: { DISPLAY: ":99" },
				})
			).output.trim();
			expect(output).toContain("copy_plane=ok");
		});

		test("PolyArc draws arcs correctly", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"polyarc_draws_arcs_correctly.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("arcs_drawn=ok");
		});
	});

test.describe
	.serial("PutImage plane_mask compliance", () => {
		test("PutImage with GC function applies correctly", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"putimage_with_gc_function_applies_correctly.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("gc_function_applied=True");
		});
	});

test.describe
	.serial("Drawable depth handling (Phase 7)", () => {
		test("CreatePixmap with depth 1 works correctly", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"createpixmap_with_depth_1_works_correctly.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("pixmap_created=True");
			expect(output).toContain("fill_ok=True");
		});

		test("Window depth matches screen root depth", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"window_depth_matches_screen_root_depth.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("root_depth=24");
			expect(output).toContain("window_depth=24");
		});
	});

test.describe
	.serial("x11perf smoke tests", () => {
		for (const { flag, label } of [
			{ flag: "-rect500", label: "500px rectangles" },
			{ flag: "-line500", label: "500px lines" },
			{ flag: "-circle500", label: "500px circles" },
			{ flag: "-copypixwin500", label: "500px pixmap-to-window copy" },
		]) {
			test(`x11perf ${flag} (${label})`, async ({ sidecarContainer }) => {
				test.setTimeout(60_000);

				const output = await execInSidecar(
					sidecarContainer,
					`x11perf ${flag} -reps 1 2>&1 || true`,
				);

				// Should not crash
				expect(output).not.toContain("Segmentation fault");
				expect(output).not.toContain("X Error");

				// Verify server is still alive after the perf test
				const alive = await execInSidecar(
					sidecarContainer,
					"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
				);
				expect(alive).toContain("alive");
			});
		}
	});
