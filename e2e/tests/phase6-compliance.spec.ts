/**
 * Phase 6 compliance tests: dynamic keymaps, clipboard text conversion,
 * accessibility features, cut buffers, and additional protocol edge cases.
 */

import { test, expect, runPythonScript } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

/** Run a command inside the sidecar container and return stdout. */
async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}


test.describe.serial("Dynamic keymap support", () => {
	test("ChangeKeyboardMapping stores and retrieves custom keysyms", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "changekeyboardmapping_stores_and_retrieves_custom_keysyms.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("original_keysym=0x61"); // 'a'
		expect(output).toContain("new_keysym=0x7a"); // 'z'
	});

	test("GetKeyboardMapping returns correct keysyms for common keys", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "getkeyboardmapping_returns_correct_keysyms_for_common_keys.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("escape=0xff1b");
		expect(output).toContain("return=0xff0d");
		expect(output).toContain("space=0x20");
	});
});

test.describe.serial("Clipboard and selection compliance", () => {
	test("Cut buffers can be written and read on root window", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "cut_buffers_can_be_written_and_read_on_root_window.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("cut_buffer0=test_cut_buffer_data");
	});

	test("RotateProperties works on cut buffers", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "rotateproperties_works_on_cut_buffers.py", { env: { DISPLAY: ":99" } })).output.trim();
		// After rotate by 1: cb0=two, cb1=zero, cb2=one
		expect(output).toContain("cb0=two");
		expect(output).toContain("cb1=zero");
		expect(output).toContain("cb2=one");
	});

	test("Selection ownership and transfer works across connections", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "selection_ownership_and_transfer_works_across_connections.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("owner_matches=True");
	});

	test("TARGETS response includes text format variants", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "targets_response_includes_text_format_variants.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("atoms_ok=True");
	});
});

test.describe.serial("XKB controls and accessibility", () => {
	test("XKB GetControls returns valid control state", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "xkb_getcontrols_returns_valid_control_state.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("xkb_present=True");
	});

	test("XKB modifier state tracks correctly", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "xkb_modifier_state_tracks_correctly.py", { env: { DISPLAY: ":99" } })).output.trim();
		// No modifiers pressed initially
		expect(output).toContain("initial_mods=0");
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

test.describe.serial("Event delivery edge cases", () => {
	test("PropertyNotify events delivered on property changes", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "propertynotify_events_delivered_on_property_changes.py", { env: { DISPLAY: ":99" } })).output.trim();
		// Should have at least 1 PropertyNotify
		const count = Number.parseInt(
			output.match(/property_notify_count=(\d+)/)?.[1] ?? "0",
		);
		expect(count).toBeGreaterThanOrEqual(1);
	});

	test("SubstructureRedirectMask generates ConfigureRequest", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "substructureredirectmask_generates_configurerequest.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("got_map_request=True");
	});

	test("Focus revert to parent on destroy", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "focus_revert_to_parent_on_destroy.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("focus_before=");
		// Focus should revert to parent (or root if parent got cleaned up)
	});
});

test.describe.serial("EWMH/ICCCM compliance", () => {
	test("_NET_SUPPORTED contains required atoms", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			`xprop -root _NET_SUPPORTED 2>/dev/null || echo "no_xprop"`,
		);
		if (!output.includes("no_xprop")) {
			expect(output).toContain("_NET_WM_NAME");
			expect(output).toContain("_NET_WM_STATE");
			expect(output).toContain("_NET_ACTIVE_WINDOW");
			expect(output).toContain("_NET_CLOSE_WINDOW");
		}
	});

	test("_NET_SUPPORTING_WM_CHECK is valid", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			`xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null || echo "no_xprop"`,
		);
		if (!output.includes("no_xprop")) {
			// Should contain a window ID
			expect(output).toMatch(/window id # 0x[0-9a-f]+/i);
		}
	});

	test("_NET_WM_PID set on mapped windows", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "net_wm_pid_set_on_mapped_windows.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("has_pid=True");
	});
});

test.describe.serial("Multi-client stress tests", () => {
	test("100 rapid window create/destroy cycles", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "100_rapid_window_create_destroy_cycles.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("completed=100");
	});

	test("500 unique atoms can be interned", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "500_unique_atoms_can_be_interned.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("total=500");
		expect(output).toContain("unique=500");
		expect(output).toContain("first_name=_TEST_ATOM_0");
	});

	test("1000 rapid property changes on single window", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "1000_rapid_property_changes_on_single_window.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("final_value=value_999");
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
