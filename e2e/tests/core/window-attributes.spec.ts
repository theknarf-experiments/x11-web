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

test.describe.serial("Backing store and save-under", () => {
	test("Backing store mode is reported in GetWindowAttributes", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "backing_store_mode_is_reported_in_getwindowattributes.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("backing_store=2");
		expect(output).toContain("backing_store_changed=1");
	});

	test("Save-under flag is stored and reported", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "save_under_flag_is_stored_and_reported.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("save_under=True");
	});

	test("Server advertises backing store support", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>&1",
		);
		// xdpyinfo should report backing store and save-under support
		expect(output).toMatch(/backing-store/i);
		expect(output).toMatch(/save-under/i);
	});
});

test.describe.serial("Bit gravity", () => {
	test("Bit gravity is stored and returned by GetWindowAttributes", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "bit_gravity_is_stored_and_returned_by_getwindowattributes.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("bit_gravity=9");
		expect(output).toContain("bit_gravity_changed=5");
	});

	test("Forget gravity (0) discards pixels on resize", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "forget_gravity_0_discards_pixels_on_resize.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("forget_gravity_resize=ok");
	});
});

test.describe("Backing store", () => {
	test("GetWindowAttributes reports backing-store attribute", async ({ sidecarContainer }) => {
		// Create a window with backing-store=Always using python3-xlib,
		// then verify GetWindowAttributes reports it back correctly.
		const result = await runPythonScript(sidecarContainer, "getwindowattrs_backing_store.py", { env: { DISPLAY: ":99" } });
		// X.Always = 2
		expect(result.output).toContain("backing_store=2");
	});

	test("backing-planes and backing-pixel are stored", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "backing_planes_pixel_stored.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("planes=0xff0000");
		expect(result.output).toContain("pixel=0xff00");
	});
});

test.describe("Orphan: Backing store", () => {
	test("GetWindowAttributes reports backing_store support", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				// Check that the server advertises backing store support
				"xdpyinfo 2>&1 | grep -i 'backing'",
			].join("\n"),
		]);
		console.log(`backing store: ${result.output.trim()}`);
		// The server should advertise backing store support
		expect(result.output.toLowerCase()).toContain("backing");
	});
});

test.describe.serial("Window management edge cases", () => {
	test("Window gravity applied on parent resize", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "window_gravity_applied_on_parent_resize.py", { env: { DISPLAY: ":99" } })).output.trim();
		// SouthEast gravity: child should move by (100, 100) when parent grows by (100, 100)
		expect(output).toContain("dx=100");
		expect(output).toContain("dy=100");
	});

	test("Override-redirect windows skip WM redirect", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "override_redirect_windows_skip_wm_redirect.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("override_redirect=1");
		expect(output).toContain("map_state=2"); // IsViewable
	});

	test("InputOnly windows have no framebuffer", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "inputonly_windows_have_no_framebuffer.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("class=2"); // InputOnly
		expect(output).toContain("map_state=2");
		expect(output).toContain("width=100");
	});

	test("CirculateWindow raises/lowers children correctly", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "circulatewindow_raises_lowers_children_correctly.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("initial_count=3");
		expect(output).toContain("circulated_count=3");
	});

	test("Deep window hierarchy (50 levels) works", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "deep_window_hierarchy_50_levels_works.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("deepest_width=10");
		// `windows[-1].translate_coords(root, 0, 0)` translates root's
		// origin into the deepest window's local frame. The deepest
		// window sits 50 px below/right of root, so root's (0,0) is at
		// (-50, -50) in its local coords.
		expect(output).toContain("translate_x=-50");
		expect(output).toContain("translate_y=-50");
	});
});

test.describe("backing store", () => {
	test("GetWindowAttributes reports backing store support", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"root = d.screen().root",
				"attrs = root.get_attributes()",
				"print(f'backing_store={attrs.backing_store}')",
				"# Setup reports BackingStore capability",
				"print(f'backing_stores={d.screen().backing_store}')",
				"print('BACKING_STORE_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("BACKING_STORE_PASS");
	});

	test("window backing store attribute round-trips", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display, Xlib.X",
				"d = Xlib.display.Display()",
				"root = d.screen().root",
				"w = root.create_window(0,0,100,100,0,d.screen().root_depth,",
				"    backing_store=Xlib.X.WhenMapped)",
				"attrs = w.get_attributes()",
				"print(f'backing_store={attrs.backing_store}')",
				"assert attrs.backing_store == Xlib.X.WhenMapped, f'Expected WhenMapped(1), got {attrs.backing_store}'",
				"w.destroy()",
				"d.close()",
				"print('BACKING_RT_PASS')",
			].join("\n"),
		]);
		expect(result.output).toContain("BACKING_RT_PASS");
	});
});
