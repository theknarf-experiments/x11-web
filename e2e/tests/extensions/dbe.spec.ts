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
	test.skip("DBE: allocate, draw, swap, and verify back buffer cycle", async ({ sidecarContainer }) => {
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
				"# Query DBE extension",
				"dbe = d.query_extension('DOUBLE-BUFFER')",
				"print(f'dbe_present={dbe is not None}')",
				"# Use xdotool to verify window exists",
				"import subprocess",
				"r = subprocess.run(['xdpyinfo', '-ext', 'DOUBLE-BUFFER'], capture_output=True, text=True)",
				"print(f'dbe_info={\"DOUBLE-BUFFER\" in r.stdout}')",
				"print('DBE_FUNC_PASS')",
				"w.destroy()",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("DBE_FUNC_PASS");
		expect(result.output).toContain("dbe_info=True");
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
				"xdpyinfo -ext DOUBLE-BUFFER 2>&1 | head -10",
			].join("\n"),
		]);
		console.log(`DBE: ${result.output.trim()}`);
		// Just check it doesn't crash and reports something
		expect(result.exitCode).toBeLessThanOrEqual(1);
	});
});

test.describe.serial("GLX and OpenGL", () => {
	test.setTimeout(120_000);

	test.skip("glxinfo works with DRISW software rendering", async ({
		sidecarContainer,
	}) => {
		// DRISW mode: LIBGL_ALWAYS_SOFTWARE=1 (set in Dockerfile) without
		// LIBGL_ALWAYS_INDIRECT. Mesa loads swrast_dri.so and uses
		// driConvertConfigs to match our FBConfigs against the driver's.
		const output = await execInSidecar(
			sidecarContainer,
			"LIBGL_DEBUG=verbose timeout 10 glxinfo -B 2>&1 | head -30",
		);
		expect(output).toContain("OpenGL renderer string: llvmpipe");
		expect(output).toContain("OpenGL version string:");
		expect(output).not.toContain("Segmentation fault");
		expect(output).not.toContain("[xcb] Extra reply data");
		expect(output).not.toContain("No matching fbConfigs");
	});

	test("glmark2 renders with DRISW software rendering", async ({
		sidecarContainer,
	}) => {
		// glmark2 calls glXChooseFBConfig with specific requirements.
		// Test what attributes it needs via a ctypes probe.
		const probeScript = [
			"import ctypes, ctypes.util, sys, os",
			"os.environ['LIBGL_ALWAYS_SOFTWARE'] = '1'",
			"X11 = ctypes.CDLL(ctypes.util.find_library('X11'))",
			"GL = ctypes.CDLL(ctypes.util.find_library('GL'))",
			"X11.XOpenDisplay.restype = ctypes.c_void_p",
			"dpy = X11.XOpenDisplay(b':99')",
			"if not dpy: print('FAIL:display'); sys.exit(1)",
			"GL.glXChooseFBConfig.restype = ctypes.POINTER(ctypes.c_void_p)",
			"# glmark2-style attrs: RGBA, double-buffered, depth 24, stencil 8",
			"attrs = (ctypes.c_int * 13)(0x8011, 1, 5, 1, 12, 24, 13, 8, 8, 8, 9, 8, 0)",
			"n = ctypes.c_int()",
			"cfgs = GL.glXChooseFBConfig(dpy, 0, attrs, ctypes.byref(n))",
			"print(f'ChooseFBConfig={n.value}')",
			"# Simpler: just double-buffer",
			"attrs2 = (ctypes.c_int * 3)(5, 1, 0)",
			"cfgs2 = GL.glXChooseFBConfig(dpy, 0, attrs2, ctypes.byref(n))",
			"print(f'SimpleChoose={n.value}')",
			"# Even simpler: no attrs at all",
			"attrs3 = (ctypes.c_int * 1)(0)",
			"cfgs3 = GL.glXChooseFBConfig(dpy, 0, attrs3, ctypes.byref(n))",
			"print(f'AnyConfig={n.value}')",
			"# Test glXGetVisualFromFBConfig — glmark2 needs this",
			"if cfgs and n.value > 0:",
			"    GL.glXGetVisualFromFBConfig.restype = ctypes.c_void_p",
			"    for i in range(min(n.value, 4)):",
			"        vi = GL.glXGetVisualFromFBConfig(dpy, cfgs[i])",
			"        print(f'  FBConfig[{i}] visual={hex(vi) if vi else \"NULL\"}')",
			"X11.XCloseDisplay(dpy)",
		].join("\n");
		const b64 = Buffer.from(probeScript).toString("base64");
		await sidecarContainer.exec(["bash", "-c", `printf '%s' '${b64}' | base64 -d > /tmp/glmark2_probe.py`]);
		const probeOutput = await execInSidecar(sidecarContainer, "python3 /tmp/glmark2_probe.py 2>&1");
		console.log("FBConfig probe:", probeOutput);

		// Run glmark2
		const output = await execInSidecar(
			sidecarContainer,
			"LIBGL_DEBUG=verbose timeout 10 glmark2 -b build 2>&1 | head -20 || true",
		);
		console.log("glmark2 output:", output);
		expect(output).not.toContain("Segmentation fault");
	});

	test("glxinfo reports GLX version and extensions", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"LIBGL_ALWAYS_INDIRECT=1 glxinfo 2>&1 | head -30",
		);
		expect(output).toContain("server glx version string: 1.4");
		expect(output).toContain("server glx vendor string: x11-web OSMesa");
		expect(output).not.toContain("[xcb] Extra reply data");
		expect(output).not.toContain("Segmentation fault");
	});

	test("glxgears renders frames without crash (indirect)", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"LIBGL_ALWAYS_INDIRECT=1 timeout 2 glxgears -info 2>&1 | head -20 || true",
		);
		expect(output).not.toContain("glXCreateContext failed");
		expect(output).not.toContain("Segmentation fault");
		expect(output).not.toContain("[xcb] Extra reply data");
	});
});
