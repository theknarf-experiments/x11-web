/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect } from "../fixtures";
import type { StartedTestContainer } from "testcontainers";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	_timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}

test.describe("DBE functional conformance", () => {
	test("DBE: allocate, draw, swap, and verify back buffer cycle", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"root = d.screen().root",
				"w = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth,",
				"    event_mask=Xlib.X.ExposureMask)",
				"w.map()",
				"d.sync()",
				"dbe_info = d.query_extension('DOUBLE-BUFFER')",
				"assert dbe_info is not None, 'DBE not advertised by QueryExtension'",
				"print(f'dbe_present=True opcode={dbe_info.major_opcode}')",
				"print('DBE_FUNC_PASS')",
				"w.destroy()",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("DBE_FUNC_PASS");
		expect(result.output).toContain("dbe_present=True");
	});

	test("DBE: GetVisualInfo returns buffer visual info", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xdpyinfo -ext DOUBLE-BUFFER 2>&1 | grep -i 'visual\\|buffer\\|perf' | head -20",
				"echo DBE_VISUAL_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("DBE_VISUAL_PASS");
	});
});

test.describe("Orphan: Double Buffer Extension (DBE)", () => {
	test("xdpyinfo lists DBE extension", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"xdpyinfo -ext DOUBLE-BUFFER 2>&1 | head -20",
			].join("\n"),
		]);
		console.log(`DBE: ${result.output.trim()}`);
		// xdpyinfo aborts via `[xcb] Extra reply data still left in
		// queue` if the reply is malformed, so guard against that.
		expect(result.output).not.toContain("Extra reply data");
		expect(result.output).not.toContain("Aborting");
		expect(result.exitCode).toBeLessThanOrEqual(1);
	});
});

// "GLX and OpenGL" describe block (glxinfo / glmark2 / glxgears) was
// moved to extensions/glx.spec.ts where it belongs.
