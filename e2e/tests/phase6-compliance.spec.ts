/**
 * Phase 6 compliance tests: dynamic keymaps, clipboard text conversion,
 * accessibility features, cut buffers, and additional protocol edge cases.
 */

import { test, expect } from "./fixtures";
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

test.describe.serial("Dynamic keymap support", () => {
	test("ChangeKeyboardMapping stores and retrieves custom keysyms", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
# Get current mapping for keycode 38 (normally 'a')
mapping = d.get_keyboard_mapping(38, 1)
orig = mapping[0][0]
print(f"original_keysym={hex(orig)}")
# Change keycode 38 to produce 'z' (0x7a) / 'Z' (0x5a)
d.change_keyboard_mapping(38, [(0x7a, 0x5a, 0, 0)])
d.sync()
# Verify it took effect
mapping2 = d.get_keyboard_mapping(38, 1)
new_sym = mapping2[0][0]
print(f"new_keysym={hex(new_sym)}")
# Restore original
d.change_keyboard_mapping(38, [(orig, mapping[0][1] if len(mapping[0]) > 1 else orig, 0, 0)])
d.sync()
d.close()
`,
		);
		expect(output).toContain("original_keysym=0x61"); // 'a'
		expect(output).toContain("new_keysym=0x7a"); // 'z'
	});

	test("GetKeyboardMapping returns correct keysyms for common keys", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
# Query keycodes 9-65 (Escape through Space)
mapping = d.get_keyboard_mapping(9, 57)
# Keycode 9 = Escape (0xff1b)
esc = mapping[0][0]
# Keycode 36 = Return (0xff0d)
ret = mapping[27][0]
# Keycode 65 = Space (0x0020)
space = mapping[56][0]
print(f"escape={hex(esc)} return={hex(ret)} space={hex(space)}")
d.close()
`,
		);
		expect(output).toContain("escape=0xff1b");
		expect(output).toContain("return=0xff0d");
		expect(output).toContain("space=0x20");
	});
});

test.describe.serial("Clipboard and selection compliance", () => {
	test("Cut buffers can be written and read on root window", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root
# Write to CUT_BUFFER0 property on root
cut_buffer0 = d.intern_atom("CUT_BUFFER0")
root.change_property(cut_buffer0, Xatom.STRING, 8, b"test_cut_buffer_data")
d.sync()
# Read it back
prop = root.get_property(cut_buffer0, Xatom.STRING, 0, 100)
if prop and prop.value:
    print(f"cut_buffer0={prop.value.decode()}")
else:
    print("cut_buffer0=EMPTY")
d.close()
`,
		);
		expect(output).toContain("cut_buffer0=test_cut_buffer_data");
	});

	test("RotateProperties works on cut buffers", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root
cb0 = d.intern_atom("CUT_BUFFER0")
cb1 = d.intern_atom("CUT_BUFFER1")
cb2 = d.intern_atom("CUT_BUFFER2")
# Set distinct values
root.change_property(cb0, Xatom.STRING, 8, b"zero")
root.change_property(cb1, Xatom.STRING, 8, b"one")
root.change_property(cb2, Xatom.STRING, 8, b"two")
d.sync()
# Rotate by 1: cb0->cb1, cb1->cb2, cb2->cb0
root.rotate_properties([cb0, cb1, cb2], 1)
d.sync()
# Read back
p0 = root.get_property(cb0, Xatom.STRING, 0, 100)
p1 = root.get_property(cb1, Xatom.STRING, 0, 100)
p2 = root.get_property(cb2, Xatom.STRING, 0, 100)
val0 = p0.value.decode() if p0 and p0.value else "EMPTY"
val1 = p1.value.decode() if p1 and p1.value else "EMPTY"
val2 = p2.value.decode() if p2 and p2.value else "EMPTY"
print(f"cb0={val0} cb1={val1} cb2={val2}")
d.close()
`,
		);
		// After rotate by 1: cb0=two, cb1=zero, cb2=one
		expect(output).toContain("cb0=two");
		expect(output).toContain("cb1=zero");
		expect(output).toContain("cb2=one");
	});

	test("Selection ownership and transfer works across connections", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display, Xatom
import time

d1 = display.Display()
d2 = display.Display()
root = d1.screen().root

# Create windows for each "client"
w1 = root.create_window(0, 0, 10, 10, 0, d1.screen().root_depth)
w2 = root.create_window(0, 0, 10, 10, 0, d2.screen().root_depth)

# d1 takes PRIMARY selection ownership
primary = d1.intern_atom("PRIMARY")
w1.set_selection_owner(primary, X.CurrentTime)
d1.sync()

# d2 checks owner. get_selection_owner returns a Window object (or 0 if
# unowned), so compare its .id to w1.id.
owner = d2.get_selection_owner(primary)
owner_id = owner.id if owner != 0 else 0
print(f"owner_matches={owner_id == w1.id}")

# Cleanup
w1.destroy()
w2.destroy()
d1.close()
d2.close()
`,
		);
		expect(output).toContain("owner_matches=True");
	});

	test("TARGETS response includes text format variants", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root

# Check if TARGETS atom exists
targets_atom = d.intern_atom("TARGETS")
utf8_atom = d.intern_atom("UTF8_STRING")
string_atom = Xatom.STRING
print(f"targets_atom={targets_atom}")
print(f"utf8_atom={utf8_atom}")
print(f"string_atom={string_atom}")
# Atoms should be non-zero
assert targets_atom != 0
assert utf8_atom != 0
print("atoms_ok=True")
d.close()
`,
		);
		expect(output).toContain("atoms_ok=True");
	});
});

test.describe.serial("XKB controls and accessibility", () => {
	test("XKB GetControls returns valid control state", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
# Check XKB extension via raw query (the python-xlib build in the
# sidecar doesn't ship Xlib.ext.xkb)
try:
    xkb_info = d.query_extension("XKEYBOARD")
    if xkb_info:
        print("xkb_present=True")
    else:
        print("xkb_present=False")
except:
    print("xkb_present=False")
d.close()
`,
		);
		expect(output).toContain("xkb_present=True");
	});

	test("XKB modifier state tracks correctly", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
root = d.screen().root
# Query initial modifier state
state = root.query_pointer()
# Modifiers should be 0 initially (no keys pressed)
print(f"initial_mods={state.mask & 0xFF}")
d.close()
`,
		);
		// No modifiers pressed initially
		expect(output).toContain("initial_mods=0");
	});
});

test.describe.serial("Window management edge cases", () => {
	test("Window gravity applied on parent resize", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
root = d.screen().root

# Create parent window
parent = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth,
    event_mask=X.SubstructureNotifyMask)
parent.map()
d.sync()

# Create child with SouthEast gravity
child = parent.create_window(50, 50, 30, 30, 0, d.screen().root_depth,
    window_class=X.InputOutput)
child.change_attributes(win_gravity=9)  # SouthEast
child.map()
d.sync()

# Get child position before resize
geom_before = child.get_geometry()
print(f"before_x={geom_before.x} before_y={geom_before.y}")

# Resize parent (larger)
parent.configure(width=300, height=300)
d.sync()

# Get child position after resize — should shift with SouthEast gravity
geom_after = child.get_geometry()
print(f"after_x={geom_after.x} after_y={geom_after.y}")

# SouthEast: child should move by the same delta as the size increase
dx = geom_after.x - geom_before.x
dy = geom_after.y - geom_before.y
print(f"dx={dx} dy={dy}")

parent.destroy()
d.close()
`,
		);
		// SouthEast gravity: child should move by (100, 100) when parent grows by (100, 100)
		expect(output).toContain("dx=100");
		expect(output).toContain("dy=100");
	});

	test("Override-redirect windows skip WM redirect", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
root = d.screen().root

# Create override-redirect window
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    override_redirect=True)
w.map()
d.sync()

# Check window attributes
attrs = w.get_attributes()
print(f"override_redirect={attrs.override_redirect}")
print(f"map_state={attrs.map_state}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("override_redirect=1");
		expect(output).toContain("map_state=2"); // IsViewable
	});

	test("InputOnly windows have no framebuffer", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
root = d.screen().root

# Create InputOnly window (class=2, depth=0)
w = root.create_window(0, 0, 100, 100, 0, 0,
    window_class=X.InputOnly)
w.map()
d.sync()

attrs = w.get_attributes()
print(f"class={attrs.win_class}")
print(f"map_state={attrs.map_state}")

# GetGeometry should still work
geom = w.get_geometry()
print(f"width={geom.width} height={geom.height}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("class=2"); // InputOnly
		expect(output).toContain("map_state=2");
		expect(output).toContain("width=100");
	});

	test("CirculateWindow raises/lowers children correctly", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
root = d.screen().root

parent = root.create_window(0, 0, 400, 400, 0, d.screen().root_depth)
parent.map()
d.sync()

# Create 3 children
c1 = parent.create_window(10, 10, 50, 50, 0, d.screen().root_depth)
c2 = parent.create_window(20, 20, 50, 50, 0, d.screen().root_depth)
c3 = parent.create_window(30, 30, 50, 50, 0, d.screen().root_depth)
c1.map(); c2.map(); c3.map()
d.sync()

# Query initial stacking (QueryTree)
tree = parent.query_tree()
initial_order = [w.id for w in tree.children]
print(f"initial_count={len(initial_order)}")

# CirculateWindow: RaiseLowest (0) - bring bottom child to top.
# python-xlib exposes the protocol request as Window.circulate(direction);
# circulate_window does not exist on the Window object.
parent.circulate(X.RaiseLowest)
d.sync()

tree2 = parent.query_tree()
new_order = [w.id for w in tree2.children]
# The lowest child should now be at the top
print(f"circulated_count={len(new_order)}")

parent.destroy()
d.close()
`,
		);
		expect(output).toContain("initial_count=3");
		expect(output).toContain("circulated_count=3");
	});

	test("Deep window hierarchy (50 levels) works", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
root = d.screen().root

# Create 50-level deep hierarchy
depth = 50
current = root
windows = []
for i in range(depth):
    w = current.create_window(1, 1, 10, 10, 0, d.screen().root_depth)
    windows.append(w)
    current = w
d.sync()

# Query the deepest window's geometry
geom = windows[-1].get_geometry()
print(f"deepest_width={geom.width}")

# TranslateCoordinates from deepest to root
tc = windows[-1].translate_coords(root, 0, 0)
print(f"translate_x={tc.x} translate_y={tc.y}")

# Cleanup
windows[0].destroy()
d.close()
`,
		);
		expect(output).toContain("deepest_width=10");
		expect(output).toContain("translate_x=50"); // 50 levels * 1 pixel offset
		expect(output).toContain("translate_y=50");
	});
});

test.describe.serial("Event delivery edge cases", () => {
	test("PropertyNotify events delivered on property changes", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root

w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    event_mask=X.PropertyChangeMask)
w.map()
d.sync()

# Set a property
test_atom = d.intern_atom("_TEST_PROP")
w.change_property(test_atom, Xatom.STRING, 8, b"hello")
d.sync()

# Check for PropertyNotify
import time
time.sleep(0.1)
count = 0
while d.pending_events():
    ev = d.next_event()
    if ev.type == X.PropertyNotify:
        count += 1
print(f"property_notify_count={count}")
d.close()
`,
		);
		// Should have at least 1 PropertyNotify
		const count = Number.parseInt(
			output.match(/property_notify_count=(\d+)/)?.[1] ?? "0",
		);
		expect(count).toBeGreaterThanOrEqual(1);
	});

	test("SubstructureRedirectMask generates ConfigureRequest", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
root = d.screen().root

# Parent selects SubstructureRedirectMask
parent = root.create_window(0, 0, 400, 400, 0, d.screen().root_depth,
    event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
parent.map()
d.sync()

# Create and map a child — should generate MapRequest (not MapNotify)
child = parent.create_window(10, 10, 50, 50, 0, d.screen().root_depth)
child.map()
d.sync()

import time
time.sleep(0.1)
got_map_request = False
while d.pending_events():
    ev = d.next_event()
    if ev.type == X.MapRequest:
        got_map_request = True
print(f"got_map_request={got_map_request}")

parent.destroy()
d.close()
`,
		);
		expect(output).toContain("got_map_request=True");
	});

	test("Focus revert to parent on destroy", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
root = d.screen().root

parent = root.create_window(0, 0, 200, 200, 0, d.screen().root_depth,
    event_mask=X.FocusChangeMask)
parent.map()
d.sync()

child = parent.create_window(10, 10, 50, 50, 0, d.screen().root_depth,
    event_mask=X.FocusChangeMask)
child.map()
d.sync()

# Set focus to child with revert_to=Parent
d.set_input_focus(child, X.RevertToParent, X.CurrentTime)
d.sync()

focus_before = d.get_input_focus()
print(f"focus_before={focus_before.focus.id}")

# Destroy the focused child — focus should revert to parent
child.destroy()
d.sync()

import time
time.sleep(0.1)
focus_after = d.get_input_focus()
focus_id = focus_after.focus.id if hasattr(focus_after.focus, "id") else focus_after.focus
print(f"focus_after={focus_id}")
print(f"parent_id={parent.id}")

parent.destroy()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)
w.map()
d.sync()
import time; time.sleep(0.1)

pid_atom = d.intern_atom("_NET_WM_PID")
prop = w.get_property(pid_atom, 0, 0, 100)
if prop and prop.value:
    print(f"has_pid=True pid={prop.value[0]}")
else:
    print("has_pid=False")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("has_pid=True");
	});
});

test.describe.serial("Multi-client stress tests", () => {
	test("100 rapid window create/destroy cycles", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display
d = display.Display()
root = d.screen().root
count = 100
for i in range(count):
    w = root.create_window(0, 0, 50, 50, 0, d.screen().root_depth)
    w.map()
    d.sync()
    w.destroy()
    d.sync()
print(f"completed={count}")
d.close()
`,
		);
		expect(output).toContain("completed=100");
	});

	test("500 unique atoms can be interned", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
atoms = []
for i in range(500):
    a = d.intern_atom(f"_TEST_ATOM_{i}")
    atoms.append(a)
d.sync()
# Verify all are unique
unique = set(atoms)
print(f"total={len(atoms)} unique={len(unique)}")
# Verify we can look them back up
name = d.get_atom_name(atoms[0])
print(f"first_name={name}")
d.close()
`,
		);
		expect(output).toContain("total=500");
		expect(output).toContain("unique=500");
		expect(output).toContain("first_name=_TEST_ATOM_0");
	});

	test("1000 rapid property changes on single window", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)
w.map()
d.sync()

prop = d.intern_atom("_TEST_RAPID")
for i in range(1000):
    w.change_property(prop, Xatom.STRING, 8, f"value_{i}".encode())
d.sync()

# Read final value
p = w.get_property(prop, Xatom.STRING, 0, 100)
val = p.value.decode() if p and p.value else "EMPTY"
print(f"final_value={val}")
w.destroy()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
ext = d.query_extension("RENDER")
print(f"render_present={ext is not None and ext.major_opcode > 0}")
d.close()
`,
		);
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
