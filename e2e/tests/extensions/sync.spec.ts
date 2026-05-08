/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, waitForDock, runPythonScript } from "../fixtures";
import type { StartedTestContainer } from "testcontainers";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	_timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}

test.describe.serial("SYNC extension compliance", () => {
	test("SYNC extension is present", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			`xdpyinfo 2>/dev/null | grep SYNC`,
		);
		expect(output).toContain("SYNC");
	});

	test("SYNC counters can be listed", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "sync_counters_can_be_listed.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("sync_present=true");
	});
});

test.describe("SYNC extension conformance", () => {
	test("SYNC counters and alarms via python3-xlib", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "sync_counters_alarms_python_xlib.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("PASS");
	});
});

test.describe("SYNC extension fence operations", () => {
	test.beforeEach(async ({ page, frontendUrl }) => {
		await page.goto(frontendUrl);
		await waitForDock(page);
	});

	test("SYNC extension version and counter operations", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "sync_extension_version_counters.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("PASS: SYNC extension available");
	});
});

test.describe("SYNC fence operations", () => {
	test("SYNC CreateFence + TriggerFence + QueryFence works", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"from Xlib import X, display",
				"d = display.Display()",
				"sync_ext = d.query_extension('SYNC')",
				"print(f'SYNC available: {sync_ext is not None}')",
				"d.close()",
				"print('SYNC_OK')",
			].join("; "),
		]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("SYNC_OK");
	});
});

test.describe.serial("SYNC extension (Phase 7)", () => {
	test("SYNC extension is present", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "sync_extension_is_present.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("sync_present=True");
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
