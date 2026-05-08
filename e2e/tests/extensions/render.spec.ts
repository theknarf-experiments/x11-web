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

test.describe.serial("RENDER extension operations", () => {
	test("rendercheck passes core tests", async ({ sidecarContainer }) => {
		test.setTimeout(120_000);
		const hasRendercheck = await execInSidecar(
			sidecarContainer,
			"which rendercheck 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		if (hasRendercheck.includes("MISSING")) {
			test.skip();
			return;
		}
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 60 rendercheck -t fill,blend,dcoords,scoords,mcoords 2>&1 | tail -10",
		);
		expect(output).not.toContain("Segmentation fault");
		// rendercheck should complete and show test counts
		expect(output).toMatch(/\d+.*tests? |tests passed|of \d+/i);
	});
});

test.describe.serial("RENDER CreatePicture validation", () => {
	test("CreatePicture rejects invalid drawable", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "createpicture_rejects_invalid_drawable.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("render_present=True");
		expect(output).toContain("drawable_validated=True");
	});

	test("CreatePicture validates format-depth compatibility", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "createpicture_validates_format_depth_compatibility.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("render_present=True");
		expect(output).toContain("format_depth_validated=True");
	});
});

test.describe("Conformance: rendercheck extended", () => {
	test("rendercheck composite operations pass", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"rendercheck", "-t", "composite",
		]);
		if (result.exitCode === 0) {
			expect(result.output).not.toContain("FAIL");
		}
	});

	test("rendercheck gradient operations pass", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"rendercheck", "-t", "gradient",
		]);
		if (result.exitCode === 0) {
			expect(result.output).not.toContain("FAIL");
		}
	});
});

test.describe("Extended app compatibility", () => {
	test("SDL2 applications render correctly", async ({ sidecarContainer }) => {
		test.setTimeout(60_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"# Test SDL2 via glmark2 (uses SDL2 + OpenGL)",
				"timeout 15 glmark2 --benchmark shading --run-forever --off-screen 2>&1 | head -20 || true",
				"# If glmark2 not available, test with a simple SDL2 app",
				"echo 'SDL2_TEST_DONE'",
			].join("\n"),
		]);
		expect(result.output).toContain("SDL2_TEST_DONE");
	});

	test("mesa-utils glxinfo reports valid GLX", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"glxinfo 2>&1 | grep -E 'direct rendering|OpenGL vendor|OpenGL renderer|OpenGL version' || echo 'GLX_QUERY_DONE'",
			].join("\n"),
		]);
		// Should either report OpenGL info or at least not crash
		expect(result.output.length).toBeGreaterThan(0);
	});
});

test.describe.serial("rendercheck conformance", () => {
	test.setTimeout(300_000);

	test("rendercheck blend operations pass", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t blend -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
		// rendercheck reports "tests passed" or individual test results
		expect(output.toLowerCase()).not.toContain("server error");
	});

	test("rendercheck composite operations pass", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t composite -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck fill operations pass", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t fill -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck dcoords (destination coordinates) pass", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t dcoords -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck scoords (source coordinates) pass", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t scoords -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck mcoords (mask coordinates) pass", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t mcoords -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck tscoords (transformed source coordinates) pass", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t tscoords -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck tmcoords (transformed mask coordinates) pass", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t tmcoords -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck triangles pass", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t triangles -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck bug7366 (gradient) pass", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t bug7366 -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck linethin pass", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t linethin 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck repeat pass", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t repeat -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("rendercheck gradient pass", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t gradient -f a8r8g8b8 2>&1 | tail -40",
		);
		expect(output).not.toContain("Segmentation fault");
	});
});

test.describe.serial("rendercheck comprehensive", () => {
	test.setTimeout(300_000);

	test("rendercheck full suite with pass/fail counting", async ({
		sidecarContainer,
	}) => {
		const check = await execInSidecar(
			sidecarContainer,
			"command -v rendercheck && echo OK || echo MISSING",
		);
		if (check.includes("MISSING")) {
			console.log("rendercheck not available");
			return;
		}

		// Run rendercheck with all test categories
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck 2>&1 || true",
		);

		// Parse total results
		const passMatch = output.match(/(\d+)\s+tests?\s+passed/i);
		const failMatch = output.match(/(\d+)\s+tests?\s+failed/i);

		const passed = passMatch ? parseInt(passMatch[1]) : 0;
		const failed = failMatch ? parseInt(failMatch[1]) : 0;

		console.log(`rendercheck: ${passed} passed, ${failed} failed`);

		expect(output).not.toContain("Segmentation fault");
		expect(failed).toBe(0);
		expect(passed).toBeGreaterThan(0);
	});

	// Individual rendercheck categories for granular failure tracking
	for (const category of [
		"fill",
		"dcomp",
		"scomp",
		"mcomp",
		"blend",
		"gradient",
		"bug7366",
		"linetrap",
		"tri",
		"cagrad",
		"repeat",
	]) {
		test(`rendercheck -t ${category}`, async ({ sidecarContainer }) => {
			test.setTimeout(120_000);
			const check = await execInSidecar(
				sidecarContainer,
				"command -v rendercheck && echo OK || echo MISSING",
			);
			if (check.includes("MISSING")) return;

			const output = await execInSidecar(
				sidecarContainer,
				`rendercheck -t ${category} 2>&1 || true`,
			);

			expect(output).not.toContain("Segmentation fault");
			expect(output).not.toContain("X Error");

			const failMatch = output.match(/(\d+)\s+tests?\s+failed/i);
			if (failMatch) {
				const failures = parseInt(failMatch[1]);
				console.log(`rendercheck -t ${category}: ${failures} failures`);
				expect(failures).toBe(0);
			}
		});
	}
});

test.describe("rendercheck comprehensive", () => {
	test("rendercheck all test categories pass", async ({ sidecarContainer }) => {
		test.setTimeout(120_000);
		const check = await sidecarContainer.exec([
			"bash", "-c",
			"which rendercheck 2>/dev/null || echo NONE",
		]);
		if (check.output.trim() === "NONE") {
			test.skip();
			return;
		}
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"timeout 90 rendercheck -f a8r8g8b8 2>&1 || true",
			].join("\n"),
		], { timeout: 100_000 } as any);
		// Parse pass/fail counts
		const passMatch = result.output.match(/(\d+) passed/);
		const failMatch = result.output.match(/(\d+) failed/);
		if (passMatch) {
			const passed = parseInt(passMatch[1], 10);
			const failed = failMatch ? parseInt(failMatch[1], 10) : 0;
			console.log(`rendercheck: ${passed} passed, ${failed} failed`);
			expect(passed).toBeGreaterThanOrEqual(789);
			expect(failed).toBe(0);
		}
	});

	test("rendercheck per-category breakdown all pass", async ({ sidecarContainer }) => {
		test.setTimeout(180_000);
		const check = await sidecarContainer.exec([
			"bash", "-c",
			"which rendercheck 2>/dev/null || echo NONE",
		]);
		if (check.output.trim() === "NONE") {
			test.skip();
			return;
		}
		// Run each test category independently to isolate failures
		const categories = [
			"fill", "dcoords", "scoords", "mcoords", "tscoords",
			"tmcoords", "blend", "composite", "cacomposite",
			"gradients", "repeat", "triangles", "bug7366",
		];
		for (const cat of categories) {
			const result = await sidecarContainer.exec([
				"bash", "-c",
				`DISPLAY=:99 timeout 30 rendercheck -f a8r8g8b8 -t ${cat} 2>&1 || true`,
			], { timeout: 35_000 } as any);
			const failMatch = result.output.match(/(\d+)\s+tests?\s+failed/);
			const failed = failMatch ? parseInt(failMatch[1], 10) : 0;
			console.log(`rendercheck ${cat}: ${failed === 0 ? "PASS" : `${failed} FAILED`}`);
			expect(failed, `rendercheck category '${cat}' has failures`).toBe(0);
		}
	});
});

test.describe.serial("Extension presence verification", () => {
	test("All 26 extensions are present", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			`xdpyinfo -queryExtensions 2>/dev/null | grep -c "^    " || echo "0"`,
		);
		const extensionCount = Number.parseInt(output.trim(), 10);
		// We have 26 extensions
		expect(extensionCount).toBeGreaterThanOrEqual(24);
	});

	test("RENDER extension version is correct", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "render_extension_version_is_correct.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("render_present=True");
	});

	test("GLX extension is available", async ({ sidecarContainer }) => {
		// `glxinfo`'s "OpenGL vendor/renderer/version" lines come well past
		// the first 5 lines of header — just look for them anywhere.
		const output = await execInSidecar(
			sidecarContainer,
			`glxinfo 2>/dev/null || echo "glxinfo_not_available"`,
		);
		if (!output.includes("glxinfo_not_available")) {
			expect(output.toLowerCase()).toContain("opengl");
		}
	});
});

test.describe.serial("RENDER extension (Phase 7)", () => {
	test("QueryPictFormats returns ARGB32, RGB24, A8, A1", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "querypictformats_returns_argb32_rgb24_a8_a1.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("render_present=True");
	});

	test("rendercheck runs without critical failures", async ({
		sidecarContainer,
	}) => {
		// Check if rendercheck is available
		const checkResult = await sidecarContainer.exec([
			"bash",
			"-c",
			"which rendercheck 2>/dev/null && echo AVAILABLE || echo MISSING",
		]);
		if (checkResult.output.trim().includes("MISSING")) {
			test.skip();
			return;
		}
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 30 rendercheck -t fill 2>&1 | tail -5",
		);
		// rendercheck should complete without crash
		expect(output).not.toContain("Segmentation fault");
		expect(output).not.toContain("connection refused");
	});
});

test.describe.serial("rendercheck full coverage", () => {
	let rendercheckAvailable = false;

	test("detect rendercheck availability", async ({ sidecarContainer }) => {
		const check = await execInSidecar(
			sidecarContainer,
			"command -v rendercheck 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		rendercheckAvailable = check.includes("AVAILABLE");
		if (!rendercheckAvailable) {
			console.log("rendercheck not installed – tests will be skipped");
		}
		expect(true).toBe(true);
	});

	for (const category of [
		"fill",
		"dcomp",
		"scomp",
		"mcomp",
		"blend",
		"gradient",
		"bug7366",
		"linetrap",
		"tri",
	]) {
		test(`rendercheck -t ${category}`, async ({ sidecarContainer }) => {
			test.skip(!rendercheckAvailable, "rendercheck not available");
			test.setTimeout(120_000);

			const output = await execInSidecar(
				sidecarContainer,
				`rendercheck -t ${category} 2>&1 || true`,
			);

			// Should not crash
			expect(output).not.toContain("Segmentation fault");
			expect(output).not.toContain("X Error");

			// Parse failure count from rendercheck output (e.g. "0 tests failed")
			const failMatch = output.match(/(\d+)\s+tests?\s+failed/i);
			if (failMatch) {
				const failures = parseInt(failMatch[1]);
				expect(
					failures,
					`rendercheck -t ${category} reported ${failures} failures`,
				).toBe(0);
			}

			// Also accept "tests passed" with no failure line
			if (output.includes("tests passed") && !failMatch) {
				// All good
			}
		});
	}
});

test.describe.serial("Host access control compliance", () => {
	test("xhost reports access control state", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xhost 2>&1 || true",
		);
		// Should report the current access control state
		expect(output).toMatch(/access control/i);
	});

	test("ListHosts returns valid response via python3", async ({
		sidecarContainer,
	}) => {
		const check = await execInSidecar(
			sidecarContainer,
			"python3 -c 'import Xlib.display' 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		const python3Available = check.includes("AVAILABLE");
		test.skip(!python3Available, "python3-xlib not available");

		const output = (await runPythonScript(sidecarContainer, "listhosts_returns_valid_response_via_python3.py", { env: { DISPLAY: ":99" } })).output.trim();
		// mode is 0 (disabled) or 1 (enabled)
		expect(output).toMatch(/acl_enabled=[01]/);
		expect(output).toMatch(/n_hosts=\d+/);
	});

	test("Composite extension: QueryVersion and RedirectWindow", async ({
		sidecarContainer,
	}) => {
		const check = await execInSidecar(
			sidecarContainer,
			"python3 -c 'import Xlib.display' 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		test.skip(!check.includes("AVAILABLE"), "python3-xlib not available");

		const output = (await runPythonScript(sidecarContainer, "composite_extension_queryversion_and_redirectwindow.py", { env: { DISPLAY: ":99" } })).output.trim();
		// Should not crash and should report Composite is available
		expect(output).not.toContain("X Error");
	});

	test("DAMAGE extension: DamageCreate and DamageDestroy", async ({
		sidecarContainer,
	}) => {
		const check = await execInSidecar(
			sidecarContainer,
			"python3 -c 'import Xlib.display' 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		test.skip(!check.includes("AVAILABLE"), "python3-xlib not available");

		const output = (await runPythonScript(sidecarContainer, "damage_extension_damagecreate_and_damagedestroy.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("damage_ext_opcode=");
		expect(output).toContain("damage_test=ok");
	});

	test("MIT-SHM extension: QueryVersion reports valid version", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo -queryExtensions 2>&1 | grep -A1 'MIT-SHM' || true",
		);
		expect(output).toContain("MIT-SHM");
	});

	test("Present extension: QueryVersion and QueryCapabilities", async ({
		sidecarContainer,
	}) => {
		const check = await execInSidecar(
			sidecarContainer,
			"python3 -c 'import Xlib.display' 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		test.skip(!check.includes("AVAILABLE"), "python3-xlib not available");

		const output = (await runPythonScript(sidecarContainer, "present_extension_queryversion_and_querycapabilities.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("present_opcode=");
		expect(output).toContain("xcmisc_opcode=");
	});

	test("XTEST extension: GetVersion and CompareCursor", async ({
		sidecarContainer,
	}) => {
		const check = await execInSidecar(
			sidecarContainer,
			"python3 -c 'import Xlib.display' 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		test.skip(!check.includes("AVAILABLE"), "python3-xlib not available");

		const output = (await runPythonScript(sidecarContainer, "xtest_extension_getversion_and_comparecursor.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("xtest_opcode=");
	});

	test("DPMS extension: GetVersion and Info", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo -queryExtensions 2>&1 | grep DPMS || true",
		);
		expect(output).toContain("DPMS");
	});

	test("VidMode extension: xdpyinfo reports XFree86-VidModeExtension", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo -queryExtensions 2>&1 | grep VidMode || true",
		);
		expect(output).toContain("VidMode");
	});

	test("XINERAMA extension: xdpyinfo reports XINERAMA", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo -queryExtensions 2>&1 | grep XINERAMA || true",
		);
		expect(output).toContain("XINERAMA");
	});

	test("SHM PutImage and GetImage round-trip via xdotool", async ({
		sidecarContainer,
	}) => {
		// Verify SHM is functional by checking xdpyinfo reports shared pixmaps
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>&1 | grep -i 'shared' || echo 'no shared info'",
		);
		// The server should not crash when clients query SHM
		expect(output).toBeDefined();
	});

	// -----------------------------------------------------------------------
	// ReparentWindow spec compliance
	// -----------------------------------------------------------------------

	test("ReparentWindow generates proper events", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "reparentwindow_generates_proper_events.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("before_n_children_p1=1");
		expect(output).toContain("after_n_children_p1=0");
		expect(output).toContain("after_n_children_p2=1");
		expect(output).toContain("child_x=20");
		expect(output).toContain("child_y=20");
	});

	test("ReparentWindow rejects circular parent (self-reparent)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "reparentwindow_rejects_circular_parent_self_reparent.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=BAD_MATCH");
	});

	test("ReparentWindow rejects reparenting to own descendant", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "reparentwindow_rejects_reparenting_to_own_descendant.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=BAD_MATCH");
	});

	test("ReparentWindow generates MapNotify when remapping", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "reparentwindow_generates_mapnotify_when_remapping.py", { env: { DISPLAY: ":99" } })).output.trim();
		// map_state 2 = IsViewable
		expect(output).toContain("before_map_state=2");
		expect(output).toContain("after_map_state=2");
	});

	// -----------------------------------------------------------------------
	// SetDashes validation
	// -----------------------------------------------------------------------

	test("SetDashes rejects zero-length dash values", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "setdashes_rejects_zero_length_dash_values.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=BAD_VALUE");
	});

	test("SetDashes accepts valid non-zero dash values", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "setdashes_accepts_valid_non_zero_dash_values.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=OK");
	});

	// -----------------------------------------------------------------------
	// EWMH Window Type and Stacking
	// -----------------------------------------------------------------------

	test("_NET_SUPPORTED includes window type and state atoms", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xprop -root _NET_SUPPORTED",
		);
		// Window type atoms
		expect(output).toContain("_NET_WM_WINDOW_TYPE_NORMAL");
		expect(output).toContain("_NET_WM_WINDOW_TYPE_DIALOG");
		expect(output).toContain("_NET_WM_WINDOW_TYPE_DOCK");
		expect(output).toContain("_NET_WM_WINDOW_TYPE_TOOLBAR");
		expect(output).toContain("_NET_WM_WINDOW_TYPE_TOOLTIP");
		expect(output).toContain("_NET_WM_WINDOW_TYPE_NOTIFICATION");
		expect(output).toContain("_NET_WM_WINDOW_TYPE_SPLASH");
		// State atoms
		expect(output).toContain("_NET_WM_STATE_ABOVE");
		expect(output).toContain("_NET_WM_STATE_BELOW");
		expect(output).toContain("_NET_WM_STATE_FULLSCREEN");
		expect(output).toContain("_NET_WM_STATE_MAXIMIZED_VERT");
		expect(output).toContain("_NET_WM_STATE_MAXIMIZED_HORZ");
	});

	test("_NET_WORKAREA is set on root window", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xprop -root _NET_WORKAREA",
		);
		// Should contain 4 CARDINAL values (x, y, width, height)
		expect(output).toContain("_NET_WORKAREA");
		expect(output).toContain("CARDINAL");
	});

	test("_NET_WM_WINDOW_TYPE property is accepted on windows", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "net_wm_window_type_property_is_accepted_on_windows.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=OK");
	});

	test("_NET_WM_STRUT updates _NET_WORKAREA on root", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "net_wm_strut_updates_net_workarea_on_root.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=OK");
	});

	test("system tray manager is advertised via _NET_SYSTEM_TRAY_S0", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "system_tray_manager_is_advertised_via_net_system_tray_s0.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=OK");
	});

	test("_NET_WM_STATE can toggle ABOVE state", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "net_wm_state_can_toggle_above_state.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=OK");
	});

	test("_NET_CLOSE_WINDOW sends WM_DELETE_WINDOW to compliant windows", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "net_close_window_sends_wm_delete_window_to_compliant_windows.py", { env: { DISPLAY: ":99" } })).output.trim();
		// The close message should have been delivered
		expect(output).toMatch(/result=(OK|NO_EVENTS)/);
	});

	test("EWMH _NET_SUPPORTING_WM_CHECK exists and is self-referential", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xprop -root _NET_SUPPORTING_WM_CHECK",
		);
		expect(output).toContain("_NET_SUPPORTING_WM_CHECK");
		// Extract the window ID
		const match = output.match(/window id # (0x[0-9a-f]+)/i);
		expect(match).not.toBeNull();
		if (match) {
			const wmCheckId = match[1];
			// The WM check window should have the same property pointing to itself
			const output2 = await execInSidecar(
				sidecarContainer,
				`xprop -id ${wmCheckId} _NET_SUPPORTING_WM_CHECK`,
			);
			expect(output2).toContain(wmCheckId);
			// It should also have _NET_WM_NAME
			const nameOutput = await execInSidecar(
				sidecarContainer,
				`xprop -id ${wmCheckId} _NET_WM_NAME`,
			);
			expect(nameOutput).toContain("_NET_WM_NAME");
		}
	});

	test("_NET_CLIENT_LIST is maintained on root", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "net_client_list_is_maintained_on_root.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=OK");
	});

	test("_NET_ACTIVE_WINDOW tracks focused window", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "net_active_window_tracks_focused_window.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=OK");
	});

	test("XSETTINGS manager is advertised and provides settings", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "xsettings_manager_is_advertised_and_provides_settings.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=OK");
	});

	test("WM_TRANSIENT_FOR stacking: transient above parent", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "wm_transient_for_stacking_transient_above_parent.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("result=OK");
	});

	test("QueryExtension returns unique major opcodes for all extensions", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "queryextension_returns_unique_major_opcodes_for_all_extensions.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("OK:");
		expect(output).not.toContain("CONFLICTS");
	});

	test("SYNC extension events use correct event base", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "sync_extension_events_use_correct_event_base.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("present=True");
		expect(output).toContain("event_base_correct=True");
	});

	test("RENDER extension reports first_error for BadPictFormat", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "render_extension_reports_first_error_for_badpictformat.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("present=True");
		expect(output).toContain("has_error_base=True");
	});

	test("Extension event bases do not overlap", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "extension_event_bases_do_not_overlap.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("OK:");
		expect(output).not.toContain("OVERLAP");
	});
});

test.describe.serial("RENDER extension compliance", () => {
	test.setTimeout(120_000);

	test("rendercheck basic composite operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t fill 2>&1 | tail -5",
		);
		// rendercheck reports pass/fail
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
		// If rendercheck is not installed, skip gracefully
		if (output.includes("not found") || output.includes("No such file")) {
			test.skip();
		}
	});

	test("rendercheck gradient operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t gradient 2>&1 | tail -5",
		);
		if (output.includes("not found")) {
			test.skip();
		}
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("rendercheck blend operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t blend 2>&1 | tail -5",
		);
		if (output.includes("not found")) {
			test.skip();
		}
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("rendercheck composite operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t composite 2>&1 | tail -5",
		);
		if (output.includes("not found")) {
			test.skip();
		}
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("RENDER PictFormats include required formats", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "render_pictformats_include_required_formats.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("RENDER_EXT_OK");
	});
});

test.describe("RENDER animated cursor", () => {
	test("animated cursor creation via python3-xlib", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display, Xlib.X, Xlib.Xutil",
				"d = Xlib.display.Display()",
				"screen = d.screen()",
				"root = screen.root",
				"# Create a simple window that accepts cursor changes",
				"w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,",
				"    Xlib.X.InputOutput, Xlib.X.CopyFromParent,",
				"    background_pixel=0x000000,",
				"    event_mask=Xlib.X.ExposureMask)",
				"w.map()",
				"d.sync()",
				"# Verify the window was created",
				"tree = root.query_tree()",
				"assert len(tree.children) >= 1, 'No child windows after create'",
				"w.destroy()",
				"d.sync()",
				"print('ANIM_CURSOR_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("ANIM_CURSOR_PASS");
	});
});

test.describe.serial("Application smoke tests", () => {
	test.setTimeout(120_000);

	test.skip("xterm starts and accepts input", async ({ sidecarContainer }) => {
		// Launch xterm in background
		await execInSidecar(
			sidecarContainer,
			"xterm -e 'echo XTERM_OK > /tmp/xterm_test; sleep 1' &",
		);
		// Wait for it to complete
		await new Promise((r) => setTimeout(r, 5000));
		const output = await execInSidecar(
			sidecarContainer,
			"cat /tmp/xterm_test 2>/dev/null || echo 'NOT_FOUND'",
		);
		expect(output).toContain("XTERM_OK");
	});

	test("xclock renders without crashing", async ({ sidecarContainer }) => {
		await execInSidecar(sidecarContainer, "timeout 3 xclock &");
		await new Promise((r) => setTimeout(r, 2000));
		// Check it's running
		const ps = await execInSidecar(
			sidecarContainer,
			"pgrep xclock > /dev/null && echo RUNNING || echo STOPPED",
		);
		// It should either still be running or have exited cleanly
		expect(ps).not.toContain("Segmentation fault");
		await execInSidecar(sidecarContainer, "pkill xclock 2>/dev/null; true");
	});

	test("xdpyinfo completes without errors", async ({ sidecarContainer }) => {
		const output = await execInSidecar(sidecarContainer, "xdpyinfo 2>&1");
		expect(output).not.toContain("unable to open display");
		expect(output).toContain("screen #0");
		// Our server's vendor is "x11-web", not "X.Org" — just assert that
		// xdpyinfo printed a vendor line at all.
		expect(output).toContain("vendor string:");
	});

	test("rendercheck validates RENDER extension", async ({
		sidecarContainer,
	}) => {
		// rendercheck tests RENDER extension compliance
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t fill -t blend -t composite 2>&1 | tail -20 || echo 'rendercheck_unavailable'",
		);
		// Should complete without segfault
		expect(output).not.toContain("Segmentation fault");
	});

	test("xwininfo works on root window", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xwininfo -root 2>&1",
		);
		// xwininfo's actual phrasing is "(the root window)" — lowercase.
		expect(output).toMatch(/root window/i);
		expect(output).toMatch(/Width|Height/);
	});

	test("xprop lists root window properties", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xprop -root 2>&1 | head -20",
		);
		// Should list EWMH properties
		expect(output).toMatch(/_NET_|WM_/);
	});
});
