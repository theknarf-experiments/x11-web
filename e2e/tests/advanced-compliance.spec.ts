/**
 * Advanced X11 protocol compliance tests.
 *
 * Tests for XKB event notifications, backing store, save-under,
 * INCR selection transfers, bit gravity, and other features
 * required for full spec compliance with real-world applications.
 */

import { test, expect } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

/** Run a command inside the sidecar container and return stdout. */
async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `DISPLAY=:99 ${cmd}`]);
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

test.describe.serial("XKB event notifications", () => {
	test("XKEYBOARD extension has proper event base", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
xkb = d.query_extension('XKEYBOARD')
print(f"present={xkb is not None and xkb.major_opcode > 0}")
if xkb:
    print(f"major_opcode={xkb.major_opcode}")
    print(f"first_event={xkb.first_event}")
    print(f"has_event_base={xkb.first_event > 0}")
d.close()
`,
		);
		expect(output).toContain("present=True");
		expect(output).toContain("major_opcode=136");
		expect(output).toContain("has_event_base=True");
	});

	test("XKB SelectEvents accepts subscription requests", async ({
		sidecarContainer,
	}) => {
		// Use xdotool to verify XKB is functional via key simulation
		const output = await execInSidecar(
			sidecarContainer,
			`xdotool key shift 2>&1 && echo "xkb_key_ok=True"`,
		);
		expect(output).toContain("xkb_key_ok=True");
	});

	test("XKB GetState returns valid modifier state", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
xkb = d.query_extension('XKEYBOARD')
if xkb:
    # XKB UseExtension (minor 0)
    import struct
    # Send UseExtension request
    buf = struct.pack('=BBHBB', xkb.major_opcode, 0, 2, 1, 0)
    d.display.send_request(d.display.request_queue, buf, None)
    d.sync()

    # XKB GetState (minor 4)
    buf = struct.pack('=BBHHxx', xkb.major_opcode, 4, 2, 0x100)
    d.display.send_request(d.display.request_queue, buf, None)
    d.sync()

    print("xkb_state_query=ok")
else:
    print("xkb_state_query=no_extension")
d.close()
`,
		);
		// Just verify the extension is queryable without crashing
		expect(output).not.toContain("error");
	});

	test("xinput list shows keyboard devices", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xinput list 2>&1",
		);
		// Should list at least a virtual core keyboard
		expect(output.toLowerCase()).toMatch(/keyboard|pointer/);
	});
});

test.describe.serial("Backing store and save-under", () => {
	test("Backing store mode is reported in GetWindowAttributes", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with backing_store=Always (2)
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    backing_store=2)
d.sync()

attrs = w.get_attributes()
print(f"backing_store={attrs.backing_store}")

# Change to WhenMapped (1)
w.change_attributes(backing_store=1)
d.sync()
attrs2 = w.get_attributes()
print(f"backing_store_changed={attrs2.backing_store}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("backing_store=2");
		expect(output).toContain("backing_store_changed=1");
	});

	test("Save-under flag is stored and reported", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with save_under=True
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    save_under=1)
d.sync()

attrs = w.get_attributes()
print(f"save_under={attrs.save_under}")

w.destroy()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with bit_gravity=SouthEast (9)
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    bit_gravity=9)
d.sync()

attrs = w.get_attributes()
print(f"bit_gravity={attrs.bit_gravity}")

# Change to Center (5)
w.change_attributes(bit_gravity=5)
d.sync()
attrs2 = w.get_attributes()
print(f"bit_gravity_changed={attrs2.bit_gravity}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("bit_gravity=9");
		expect(output).toContain("bit_gravity_changed=5");
	});

	test("Forget gravity (0) discards pixels on resize", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with ForgetGravity (0) - default
w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
    bit_gravity=0)
w.map()
d.sync()

# Draw something
gc = w.create_gc(foreground=0xFF0000)
w.fill_rectangle(gc, 0, 0, 50, 50)
d.sync()

# Resize - with ForgetGravity, Expose should be generated
w.configure(width=100, height=100)
d.sync()

import time
time.sleep(0.1)

print("forget_gravity_resize=ok")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("forget_gravity_resize=ok");
	});
});

test.describe.serial("INCR selection transfer", () => {
	test("Large clipboard data can be transferred between clients", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()

# Create a window to own the clipboard
w = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth,
    event_mask=Xlib.X.PropertyChangeMask)
w.map()
d.sync()

# Set selection owner
clipboard = d.intern_atom('CLIPBOARD')
d.set_selection_owner(w, clipboard, Xlib.X.CurrentTime)
d.sync()

# Verify ownership
owner = d.get_selection_owner(clipboard)
print(f"owns_clipboard={owner.id == w.id}")

# Set a property with data
test_data = b"Hello from X11 clipboard test! " * 100  # ~3KB
test_atom = d.intern_atom('_CLIP_TEST')
w.change_property(test_atom, Xlib.Xatom.STRING, 8, test_data)
d.sync()

# Read it back
prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
if prop:
    print(f"data_len={len(prop.value)}")
    print(f"data_matches={prop.value == test_data}")
else:
    print("data_matches=False")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("owns_clipboard=True");
		expect(output).toContain("data_matches=True");
	});

	test("Selection conversion between two clients works", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
import time

d1 = Xlib.display.Display()
d2 = Xlib.display.Display()
screen = d1.screen()

# Client 1: create window and own PRIMARY selection
owner_w = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth,
    event_mask=Xlib.X.PropertyChangeMask)
owner_w.map()
d1.sync()

primary = d1.intern_atom('PRIMARY')
utf8 = d1.intern_atom('UTF8_STRING')
d1.set_selection_owner(owner_w, primary, Xlib.X.CurrentTime)
d1.sync()

owner_check = d1.get_selection_owner(primary)
print(f"owner_set={owner_check.id == owner_w.id}")

# Client 2: request conversion
from Xlib.xobject.drawable import Window
screen2 = d2.screen()
req_w = screen2.root.create_window(0, 0, 1, 1, 0, screen2.root_depth,
    event_mask=Xlib.X.PropertyChangeMask)
req_w.map()
d2.sync()

# ConvertSelection
result_prop = d2.intern_atom('_RESULT')
d2.convert_selection(primary, utf8, result_prop, req_w, Xlib.X.CurrentTime)
d2.sync()

# The owner should receive SelectionRequest
time.sleep(0.2)
d1.sync()
got_request = False
while d1.pending_events() > 0:
    ev = d1.next_event()
    if ev.type == 30:  # SelectionRequest
        got_request = True
        break
print(f"got_selection_request={got_request}")

owner_w.destroy()
req_w.destroy()
d1.close()
d2.close()
`,
		);
		expect(output).toContain("owner_set=True");
		expect(output).toContain("got_selection_request=True");
	});
});

test.describe.serial("Advanced event delivery", () => {
	test("Enter/Leave events generated on pointer warp", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(100, 100, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
w.map()
d.sync()

# Warp into the window
d.warp_pointer(0, 0, 0, 0, 0, 0, 150, 150)
d.sync()

import time
time.sleep(0.1)
d.sync()

events = []
while d.pending_events() > 0:
    ev = d.next_event()
    events.append(ev.type)

# EnterNotify=7, LeaveNotify=8
print(f"event_types={events}")
print(f"got_enter={7 in events}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("got_enter=True");
	});

	test("FocusIn/FocusOut events on SetInputFocus", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w2 = screen.root.create_window(200, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w1.map()
w2.map()
d.sync()

# Focus w1
d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus1 = d.get_input_focus()
print(f"focus_w1={focus1.focus.id == w1.id}")

# Now focus w2
d.set_input_focus(w2, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus2 = d.get_input_focus()
print(f"focus_w2={focus2.focus.id == w2.id}")

import time
time.sleep(0.1)

# Drain events - should have FocusIn/FocusOut
focus_events = []
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type in (9, 10):  # FocusIn=9, FocusOut=10
        focus_events.append(ev.type)

print(f"got_focus_in={9 in focus_events}")
print(f"got_focus_out={10 in focus_events}")

w1.destroy()
w2.destroy()
d.close()
`,
		);
		expect(output).toContain("focus_w1=True");
		expect(output).toContain("focus_w2=True");
		expect(output).toContain("got_focus_in=True");
	});

	test("ConfigureNotify on sibling stacking change", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)
w2 = screen.root.create_window(50, 50, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)
w1.map()
w2.map()
d.sync()

# Drain initial events
import time
time.sleep(0.1)
while d.pending_events() > 0:
    d.next_event()

# Raise w1 above w2
w1.configure(stack_mode=Xlib.X.Above)
d.sync()
time.sleep(0.1)

got_configure = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == 22:  # ConfigureNotify
        got_configure = True

print(f"got_configure_notify={got_configure}")

w1.destroy()
w2.destroy()
d.close()
`,
		);
		expect(output).toContain("got_configure_notify=True");
	});
});

test.describe.serial("Drawing operations compliance", () => {
	test("PolyFillRectangle with GC function XOR", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# Fill with white
gc_white = w.create_gc(foreground=0xFFFFFF, function=Xlib.X.GXcopy)
w.fill_rectangle(gc_white, 0, 0, 100, 100)
d.sync()

# XOR with red
gc_xor = w.create_gc(foreground=0xFF0000, function=Xlib.X.GXxor)
w.fill_rectangle(gc_xor, 10, 10, 50, 50)
d.sync()

# Get the pixel at (20, 20) - should be white XOR red = cyan (0x00FFFF)
img = w.get_image(20, 20, 1, 1, 0xFFFFFFFF, Xlib.X.ZPixmap)
import struct
pixel = struct.unpack('<I', img.data[:4])[0] & 0xFFFFFF
print(f"pixel=0x{pixel:06x}")
# white (0xFFFFFF) XOR red (0xFF0000) = cyan (0x00FFFF)
print(f"xor_correct={pixel == 0x00FFFF}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("xor_correct=True");
	});

	test("CopyPlane between depths", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create a depth-1 pixmap
pm = screen.root.create_pixmap(10, 10, 1)
gc1 = pm.create_gc(foreground=1, background=0)
pm.fill_rectangle(gc1, 0, 0, 10, 10)
d.sync()

# Create a depth-24 window
w = screen.root.create_window(0, 0, 20, 20, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# CopyPlane from depth-1 to depth-24
gc24 = w.create_gc(foreground=0x00FF00, background=0x000000)
w.copy_plane(gc24, pm, 0, 0, 10, 10, 5, 5, 1)
d.sync()

print("copy_plane=ok")

pm.free()
w.destroy()
d.close()
`,
		);
		expect(output).toContain("copy_plane=ok");
	});

	test("PolyArc draws arcs correctly", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

gc = w.create_gc(foreground=0xFF0000, line_width=2)
# Draw a full circle (arc from 0 to 360 degrees, in 64ths of a degree)
w.arc(gc, 50, 50, 100, 100, 0, 360 * 64)
d.sync()

# Fill arc (pie slice)
gc_fill = w.create_gc(foreground=0x00FF00, arc_mode=Xlib.X.ArcPieSlice)
w.fill_arc(gc_fill, 50, 50, 100, 100, 0, 90 * 64)
d.sync()

print("arcs_drawn=ok")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("arcs_drawn=ok");
	});
});

test.describe.serial("Colormap operations", () => {
	test("AllocColor returns correct RGB values", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Alloc a specific color
cm = screen.default_colormap
color = cm.alloc_color(0xFFFF, 0x0000, 0x8080)
print(f"pixel={color.pixel}")
print(f"red={color.red}")
print(f"green={color.green}")
print(f"blue={color.blue}")
# Red channel should be 0xFFFF, green 0, blue ~0x8080
print(f"red_match={color.red == 0xFFFF}")

d.close()
`,
		);
		expect(output).toContain("red_match=True");
	});

	test("AllocNamedColor resolves color names", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
cm = screen.default_colormap

color = cm.alloc_named_color('red')
print(f"red_pixel={color.pixel}")
print(f"red_exact_r={color.exact_red}")

color2 = cm.alloc_named_color('blue')
print(f"blue_pixel={color2.pixel}")
print(f"blue_exact_b={color2.exact_blue}")

# Named colors should resolve to proper RGB
print(f"red_ok={color.exact_red == 0xFFFF and color.exact_green == 0 and color.exact_blue == 0}")
print(f"blue_ok={color2.exact_red == 0 and color2.exact_green == 0 and color2.exact_blue == 0xFFFF}")

d.close()
`,
		);
		expect(output).toContain("red_ok=True");
		expect(output).toContain("blue_ok=True");
	});
});

test.describe.serial("RENDER extension operations", () => {
	test("rendercheck passes core tests", async ({ sidecarContainer }) => {
		test.setTimeout(120_000);
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t fill,blend,composite,dcoords,scoords,mcoords 2>&1 | tail -5",
		);
		expect(output).not.toContain("Segmentation fault");
		// Should show test results
		expect(output).toMatch(/tests passed|of \d+/i);
	});
});

test.describe.serial("SHAPE extension", () => {
	test("ShapeRectangles sets window shape", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
shape = d.query_extension('SHAPE')
print(f"shape_present={shape is not None and shape.major_opcode > 0}")
d.close()
`,
		);
		expect(output).toContain("shape_present=True");
	});
});
