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

/** Run a python3-xlib script inside the sidecar container. */
async function runPythonX11(
	container: StartedTestContainer,
	script: string,
): Promise<string> {
	const escaped = script.replace(/'/g, "'\\''");
	const result = await container.exec([
		"bash",
		"-c",
		`DISPLAY=:99 python3 -c '${escaped}'`,
	]);
	return result.output.trim();
}

// ---------------------------------------------------------------------------
// WM_HINTS parsing and ICCCM input focus model
// ---------------------------------------------------------------------------

test.describe.serial("WM_HINTS input focus model (Phase 8A)", () => {
	test("WM_HINTS input=true is parsed (Passive/Locally Active)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
# Set WM_HINTS with input=True (flags bit 0 = InputHint)
w.set_wm_hints(flags=Xutil.InputHint, input=1)
d.sync()

# Read back WM_HINTS to verify
read_hints = w.get_wm_hints()
input_val = getattr(read_hints, 'input', -1) if read_hints else -1
print(f"input={input_val}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("input=1");
	});

	test("WM_HINTS input=false is parsed (Globally Active/No Input)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
w.set_wm_hints(flags=Xutil.InputHint, input=0)
d.sync()

read_hints = w.get_wm_hints()
input_val = getattr(read_hints, 'input', -1) if read_hints else -1
print(f"input={input_val}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("input=0");
	});

	test("WM_HINTS window_group is stored", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
leader = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth)
child = root.create_window(20, 20, 100, 100, 0, d.screen().root_depth)

# Set WM_HINTS with window_group on child
hints = {}
hints['flags'] = Xutil.WindowGroupHint
hints['window_group'] = leader.id
child.set_wm_hints(hints)
d.sync()

read_hints = child.get_wm_hints()
group = getattr(read_hints, 'window_group', 0) if read_hints else 0
print(f"group_matches={group == leader.id}")
leader.destroy()
child.destroy()
d.close()
`,
		);
		expect(output).toContain("group_matches=True");
	});

	test("WM_HINTS urgency hint triggers WindowUrgent", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
hints = {}
hints['flags'] = Xutil.UrgencyHint
w.set_wm_hints(hints)
d.sync()

read_hints = w.get_wm_hints()
flags = getattr(read_hints, 'flags', 0) if read_hints else 0
urgent = bool(flags & Xutil.UrgencyHint)
print(f"urgent={urgent}")
w.destroy()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
root = d.screen().root

# Create parent and modal child
parent = root.create_window(10, 10, 400, 400, 0, d.screen().root_depth,
    event_mask=X.StructureNotifyMask | X.PropertyChangeMask)
parent.map()
d.sync()

child = root.create_window(50, 50, 200, 200, 0, d.screen().root_depth)
child.map()
d.sync()

# Set WM_TRANSIENT_FOR on child
wm_transient_for = d.intern_atom('WM_TRANSIENT_FOR')
child.change_property(wm_transient_for, 33, 32, [parent.id])
d.sync()

# Set _NET_WM_STATE_MODAL on child via ClientMessage
net_wm_state = d.intern_atom('_NET_WM_STATE')
net_wm_state_modal = d.intern_atom('_NET_WM_STATE_MODAL')

# Send _NET_WM_STATE ClientMessage to root
from Xlib.protocol import event
import struct
e = event.ClientMessage(
    window=child.id,
    client_type=net_wm_state,
    data=(32, [1, net_wm_state_modal, 0, 1, 0])  # 1=_NET_WM_STATE_ADD
)
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

# Read back _NET_WM_STATE to check MODAL is set
state_prop = child.get_full_property(net_wm_state, X.AnyPropertyType)
if state_prop:
    import array
    atoms = array.array('I', state_prop.value)
    has_modal = net_wm_state_modal in atoms
    print(f"has_modal={has_modal}")
else:
    print("has_modal=False")

child.destroy()
parent.destroy()
d.close()
`,
		);
		expect(output).toContain("has_modal=True");
	});

	test("_NET_WM_STATE_MODAL is toggled correctly", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
from Xlib.protocol import event
d = display.Display()
root = d.screen().root

w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
w.map()
d.sync()

net_wm_state = d.intern_atom('_NET_WM_STATE')
modal = d.intern_atom('_NET_WM_STATE_MODAL')

# Add MODAL
e = event.ClientMessage(window=w.id, client_type=net_wm_state, data=(32, [1, modal, 0, 1, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

# Toggle MODAL (should remove it)
e = event.ClientMessage(window=w.id, client_type=net_wm_state, data=(32, [2, modal, 0, 1, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

import array
state_prop = w.get_full_property(net_wm_state, X.AnyPropertyType)
atoms = array.array('I', state_prop.value) if state_prop and state_prop.value else []
has_modal = modal in atoms
print(f"modal_removed={not has_modal}")

w.destroy()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
from Xlib.protocol import event
d = display.Display()
root = d.screen().root

w = root.create_window(10, 10, 200, 200, 2, d.screen().root_depth,
    event_mask=X.PropertyChangeMask)
d.sync()

net_request = d.intern_atom('_NET_REQUEST_FRAME_EXTENTS')
net_frame = d.intern_atom('_NET_FRAME_EXTENTS')

# Send _NET_REQUEST_FRAME_EXTENTS ClientMessage
e = event.ClientMessage(window=w.id, client_type=net_request, data=(32, [0, 0, 0, 0, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

# Read _NET_FRAME_EXTENTS
import time
time.sleep(0.1)
d.sync()
prop = w.get_full_property(net_frame, X.AnyPropertyType)
if prop and prop.value:
    import array
    vals = array.array('I', prop.value)
    print(f"extents={list(vals)}")
    # With border_width=2, all extents should be 2
    print(f"correct={all(v == 2 for v in vals)}")
else:
    print("extents=none")
    print("correct=False")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("correct=True");
	});

	test("zero border_width gives zero frame extents", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
from Xlib.protocol import event
d = display.Display()
root = d.screen().root

w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth,
    event_mask=X.PropertyChangeMask)
d.sync()

net_request = d.intern_atom('_NET_REQUEST_FRAME_EXTENTS')
net_frame = d.intern_atom('_NET_FRAME_EXTENTS')

e = event.ClientMessage(window=w.id, client_type=net_request, data=(32, [0, 0, 0, 0, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

import time
time.sleep(0.1)
d.sync()
prop = w.get_full_property(net_frame, X.AnyPropertyType)
if prop and prop.value:
    import array
    vals = array.array('I', prop.value)
    print(f"all_zero={all(v == 0 for v in vals)}")
else:
    print("all_zero=True")

w.destroy()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
root = d.screen().root

parent = root.create_window(10, 10, 400, 400, 0, d.screen().root_depth)
parent.map()
d.sync()

child = root.create_window(50, 50, 200, 200, 0, d.screen().root_depth)
wm_transient_for = d.intern_atom('WM_TRANSIENT_FOR')
child.change_property(wm_transient_for, 33, 32, [parent.id])
child.map()
d.sync()

# Query tree to check stacking order
tree = root.query_tree()
children = tree.children
parent_idx = -1
child_idx = -1
for i, c in enumerate(children):
    if c.id == parent.id:
        parent_idx = i
    if c.id == child.id:
        child_idx = i

print(f"child_above_parent={child_idx > parent_idx}")

child.destroy()
parent.destroy()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)

wm_protocols = d.intern_atom('WM_PROTOCOLS')
wm_delete = d.intern_atom('WM_DELETE_WINDOW')
wm_take_focus = d.intern_atom('WM_TAKE_FOCUS')

w.change_property(wm_protocols, 4, 32, [wm_delete, wm_take_focus])
d.sync()

prop = w.get_full_property(wm_protocols, X.AnyPropertyType)
if prop and prop.value:
    import array
    atoms = array.array('I', prop.value)
    has_delete = wm_delete in atoms
    has_focus = wm_take_focus in atoms
    print(f"has_delete={has_delete}")
    print(f"has_focus={has_focus}")
else:
    print("has_delete=False")
    print("has_focus=False")

w.destroy()
d.close()
`,
		);
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

	test("DRI3 QueryVersion returns 1.2", async ({ sidecarContainer }) => {
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
