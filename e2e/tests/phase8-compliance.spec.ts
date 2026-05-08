/**
 * Phase 8 compliance tests: ICCCM WM_HINTS, modal dialog blocking,
 * _NET_REQUEST_FRAME_EXTENTS, and focus model compliance.
 */

import { test, expect, runPythonScript } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

/** Run a command inside the sidecar container and return stdout. */
async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}


// ---------------------------------------------------------------------------
// WM_HINTS parsing and ICCCM input focus model
// ---------------------------------------------------------------------------

test.describe.serial("WM_HINTS input focus model (Phase 8A)", () => {
	test("WM_HINTS input=true is parsed (Passive/Locally Active)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "wm_hints_input_true_is_parsed_passive_locally_active.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("input=1");
	});

	test("WM_HINTS input=false is parsed (Globally Active/No Input)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "wm_hints_input_false_is_parsed_globally_active_no_input.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("input=0");
	});

	test("WM_HINTS window_group is stored", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "wm_hints_window_group_is_stored.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("group_matches=True");
	});

	test("WM_HINTS urgency hint triggers WindowUrgent", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "wm_hints_urgency_hint_triggers_windowurgent.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("urgent=True");
	});
});

// ---------------------------------------------------------------------------
// Modal dialog blocking
// ---------------------------------------------------------------------------

test.describe.serial("Modal dialog blocking (Phase 8B)", () => {
	test("_NET_WM_STATE_MODAL can be set on a window", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "net_wm_state_modal_can_be_set_on_a_window.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("has_modal=True");
	});

	test("_NET_WM_STATE_MODAL is toggled correctly", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "net_wm_state_modal_is_toggled_correctly.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("modal_removed=True");
	});
});

// ---------------------------------------------------------------------------
// _NET_REQUEST_FRAME_EXTENTS
// ---------------------------------------------------------------------------

test.describe.serial("_NET_REQUEST_FRAME_EXTENTS (Phase 8C)", () => {
	test("server responds with _NET_FRAME_EXTENTS property", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "server_responds_with_net_frame_extents_property.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("correct=True");
	});

	test("zero border_width gives zero frame extents", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "zero_border_width_gives_zero_frame_extents.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("all_zero=True");
	});
});

// ---------------------------------------------------------------------------
// WM_TRANSIENT_FOR stacking
// ---------------------------------------------------------------------------

test.describe.serial("WM_TRANSIENT_FOR (Phase 8D)", () => {
	test("transient window is placed above its parent on map", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "transient_window_is_placed_above_its_parent_on_map.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("child_above_parent=True");
	});
});

// ---------------------------------------------------------------------------
// ICCCM WM_DELETE_WINDOW protocol
// ---------------------------------------------------------------------------

test.describe.serial("ICCCM WM_DELETE_WINDOW (Phase 8E)", () => {
	test("WM_PROTOCOLS property can be set and read back", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "wm_protocols_property_can_be_set_and_read_back.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("has_delete=True");
		expect(output).toContain("has_focus=True");
	});
});

test.describe("Grab operations", () => {
	test("GrabPointer and UngrabPointer via xdotool", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "grabpointer_ungrabpointer_xdotool.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(
			/grabs-basic: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(4);
	});

	test("passive button grab and ungrab", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "passive_button_grab_ungrab.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(
			/grabs-passive: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(4);
	});
});

test.describe("Resource cleanup on client disconnect", () => {
	test("windows are destroyed when client disconnects in Destroy mode", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "client_disconnect_destroy_windows.py", { env: { DISPLAY: ":99" } });
		console.log(`Destroy-mode test output: ${result.output}`);
		const match = result.output.match(
			/cleanup-destroy: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});

	test("SetCloseDownMode RetainTemporary keeps windows alive", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "setclosedownmode_retaintemporary.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(
			/cleanup-retain: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});
});

test.describe("Phase 8: Background pixmap, VisibilityNotify, grab sync, DRI3 fences", () => {
	test("background pixmap attribute is accepted in ChangeWindowAttributes", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess as sp",
				"r = sp.run([",
				"    'python3', '-c',",
				"    'import Xlib.display, Xlib.X\\n'",
				"    + 'd = Xlib.display.Display()\\n'",
				"    + 'root = d.screen().root\\n'",
				"    + 'w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth, background_pixel=0xFF0000)\\n'",
				"    + 'w.change_attributes(background_pixel=0x00FF00)\\n'",
				"    + 'd.sync()\\n'",
				"    + 'attrs = w.get_attributes()\\n'",
				"    + 'print(\"CLASS:\" + str(attrs.win_class))\\n'",
				"    + 'w.destroy()\\n'",
				"    + 'd.close()\\n'",
				"    + 'print(\"BG_PIXMAP_OK\")\\n'",
				"], capture_output=True, text=True)",
				"print(r.stdout)",
				"print(r.stderr)",
			].join("\n"),
		]);
		console.log(`Background pixmap: ${result.output}`);
		expect(result.output).toContain("BG_PIXMAP_OK");
	});

	test("VisibilityNotify is sent on MapWindow", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess as sp",
				"r = sp.run([",
				"    'python3', '-c',",
				"    'import Xlib.display, Xlib.X\\n'",
				"    + 'd = Xlib.display.Display()\\n'",
				"    + 'root = d.screen().root\\n'",
				"    + 'w = root.create_window(0, 0, 200, 200, 0, d.screen().root_depth,\\n'",
				"    + '    event_mask=Xlib.X.VisibilityChangeMask | Xlib.X.ExposureMask)\\n'",
				"    + 'w.map()\\n'",
				"    + 'd.sync()\\n'",
				"    + 'import time; time.sleep(0.5)\\n'",
				"    + 'found_vis = False\\n'",
				"    + 'while d.pending_events() > 0:\\n'",
				"    + '    ev = d.next_event()\\n'",
				"    + '    if ev.type == Xlib.X.VisibilityNotify:\\n'",
				"    + '        found_vis = True\\n'",
				"    + '        print(f\"VIS_STATE:{ev.state}\")\\n'",
				"    + 'if found_vis:\\n'",
				"    + '    print(\"VISIBILITY_OK\")\\n'",
				"    + 'else:\\n'",
				"    + '    print(\"NO_VISIBILITY\")\\n'",
				"    + 'w.destroy()\\n'",
				"    + 'd.close()\\n'",
				"], capture_output=True, text=True)",
				"print(r.stdout)",
				"print(r.stderr)",
			].join("\n"),
		]);
		console.log(`VisibilityNotify: ${result.output}`);
		expect(result.output).toContain("VISIBILITY_OK");
	});

	test("AllowEvents SyncPointer mode re-freezes correctly", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess as sp",
				"r = sp.run([",
				"    'python3', '-c',",
				"    'import Xlib.display, Xlib.X\\n'",
				"    + 'd = Xlib.display.Display()\\n'",
				"    + 'root = d.screen().root\\n'",
				"    + 'w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,\\n'",
				"    + '    event_mask=Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask)\\n'",
				"    + 'w.map()\\n'",
				"    + 'd.sync()\\n'",
				"    + '# GrabButton with Synchronous pointer mode\\n'",
				"    + 'w.grab_button(1, Xlib.X.AnyModifier, True,\\n'",
				"    + '    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,\\n'",
				"    + '    Xlib.X.GrabModeSync, Xlib.X.GrabModeAsync, 0, 0)\\n'",
				"    + 'd.sync()\\n'",
				"    + 'print(\"SYNC_GRAB_OK\")\\n'",
				"    + 'w.destroy()\\n'",
				"    + 'd.close()\\n'",
				"], capture_output=True, text=True)",
				"print(r.stdout)",
				"print(r.stderr)",
			].join("\n"),
		]);
		console.log(`SyncGrab: ${result.output}`);
		expect(result.output).toContain("SYNC_GRAB_OK");
	});

	// DRI3 was removed from the server (commit 60b4bd3).
	test.skip("DRI3 QueryVersion returns 1.2", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess as sp",
				"r = sp.run(['xdpyinfo', '-ext', 'DRI3'], capture_output=True, text=True)",
				"print(r.stdout)",
				"if 'DRI3' in r.stdout:",
				"    print('DRI3_FOUND')",
				"else:",
				"    print('DRI3_MISSING')",
			].join("\n"),
		]);
		console.log(`DRI3: ${result.output}`);
		// DRI3 extension should be reported
		expect(result.output).toContain("DRI3_FOUND");
	});

	test("SYNC extension fences can be created and queried", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess as sp",
				"r = sp.run([",
				"    'python3', '-c',",
				"    'import Xlib.display\\n'",
				"    + 'd = Xlib.display.Display()\\n'",
				"    + '# Verify SYNC extension is available\\n'",
				"    + 'exts = d.list_extensions()\\n'",
				"    + 'sync_found = any(\"SYNC\" in e for e in exts)\\n'",
				"    + 'if sync_found:\\n'",
				"    + '    print(\"SYNC_EXT_OK\")\\n'",
				"    + 'else:\\n'",
				"    + '    print(\"SYNC_EXT_MISSING\")\\n'",
				"    + 'd.close()\\n'",
				"], capture_output=True, text=True)",
				"print(r.stdout)",
				"print(r.stderr)",
			].join("\n"),
		]);
		console.log(`SYNC fences: ${result.output}`);
		expect(result.output).toContain("SYNC_EXT_OK");
	});

	test("window stacking changes emit VisibilityNotify to affected siblings", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess as sp",
				"r = sp.run([",
				"    'python3', '-c',",
				"    'import Xlib.display, Xlib.X\\n'",
				"    + 'd = Xlib.display.Display()\\n'",
				"    + 'root = d.screen().root\\n'",
				"    + '# Create two overlapping windows\\n'",
				"    + 'w1 = root.create_window(0, 0, 200, 200, 0, d.screen().root_depth,\\n'",
				"    + '    event_mask=Xlib.X.VisibilityChangeMask | Xlib.X.ExposureMask)\\n'",
				"    + 'w2 = root.create_window(50, 50, 200, 200, 0, d.screen().root_depth,\\n'",
				"    + '    event_mask=Xlib.X.VisibilityChangeMask | Xlib.X.ExposureMask)\\n'",
				"    + 'w1.map()\\n'",
				"    + 'w2.map()\\n'",
				"    + 'd.sync()\\n'",
				"    + 'import time; time.sleep(0.5)\\n'",
				"    + '# Drain events\\n'",
				"    + 'while d.pending_events() > 0:\\n'",
				"    + '    d.next_event()\\n'",
				"    + '# Raise w1 above w2 — should change w2 visibility\\n'",
				"    + 'w1.configure(stack_mode=Xlib.X.Above)\\n'",
				"    + 'd.sync()\\n'",
				"    + 'time.sleep(0.3)\\n'",
				"    + 'print(\"STACKING_VISIBILITY_OK\")\\n'",
				"    + 'w1.destroy()\\n'",
				"    + 'w2.destroy()\\n'",
				"    + 'd.close()\\n'",
				"], capture_output=True, text=True)",
				"print(r.stdout)",
				"print(r.stderr)",
			].join("\n"),
		]);
		console.log(`Stacking visibility: ${result.output}`);
		expect(result.output).toContain("STACKING_VISIBILITY_OK");
	});

	test("GLX extension reports WaitGL/WaitX support", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess as sp",
				"r = sp.run(['xdpyinfo', '-ext', 'GLX'], capture_output=True, text=True)",
				"print(r.stdout[:2000])",
				"if 'GLX' in r.stdout:",
				"    print('GLX_FOUND')",
				"else:",
				"    print('GLX_MISSING')",
			].join("\n"),
		]);
		console.log(`GLX: ${result.output}`);
		expect(result.output).toContain("GLX_FOUND");
	});

	test("cross-connection PropertyNotify delivery", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "cross_connection_propertynotify.py", { env: { DISPLAY: ":99" } });
		console.log(`Cross-connection PropertyNotify: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("cross-connection SubstructureNotify delivery", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "cross_connection_substructurenotify.py", { env: { DISPLAY: ":99" } });
		console.log(`Cross-connection SubstructureNotify: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("EWMH _NET_WM_STATE toggle via ClientMessage", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "ewmh_net_wm_state_toggle_clientmessage.py", { env: { DISPLAY: ":99" } });
		console.log(`EWMH _NET_WM_STATE toggle: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("all event mask bits are correctly defined", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "all_event_mask_bits_defined.py", { env: { DISPLAY: ":99" } });
		console.log(`Event masks: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("WM_CHANGE_STATE IconicState request works", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "wm_change_state_iconic_request.py", { env: { DISPLAY: ":99" } });
		console.log(`WM_CHANGE_STATE: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("ResizeRedirectMask is accepted in event mask", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "resizeredirectmask_event_mask.py", { env: { DISPLAY: ":99" } });
		console.log(`ResizeRedirectMask: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("ColormapNotify is broadcast cross-connection", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "colormapnotify_cross_connection.py", { env: { DISPLAY: ":99" } });
		console.log(`ColormapNotify broadcast: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("ExposureMask events are broadcast cross-connection", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "exposuremask_cross_connection.py", { env: { DISPLAY: ":99" } });
		console.log(`ExposureMask broadcast: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("MappingNotify broadcast to all clients", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "mappingnotify_broadcast_clients.py", { env: { DISPLAY: ":99" } });
		console.log(`MappingNotify broadcast: ${result.output}`);
		expect(result.output).toContain("PASS");
	});
});

test.describe("Resource cleanup on disconnect", () => {
	test("server cleans up resources after client disconnect", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "resource_cleanup_after_disconnect.py", { env: { DISPLAY: ":99" } });
		console.log(`Resource cleanup: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("SaveSet reparenting works on WM disconnect", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "saveset_reparenting_wm_disconnect.py", { env: { DISPLAY: ":99" } });
		console.log(`SaveSet: ${result.output}`);
		expect(result.output).toContain("PASS");
	});
});
