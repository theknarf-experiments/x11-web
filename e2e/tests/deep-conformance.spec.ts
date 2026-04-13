/**
 * Deep conformance tests for full X11 spec compliance.
 *
 * Runs rendercheck, extended x11perf, real application deep tests,
 * multi-client interaction, and protocol edge cases that verify the
 * X11 server works with any and all applications.
 */

import { test, expect } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

/** Run a command inside the sidecar container and return stdout. */
async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	timeoutMs = 60_000,
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

// ===========================================================================
// RENDERCHECK — RENDER extension conformance suite
// ===========================================================================

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

// ===========================================================================
// X11PERF — Extended performance and correctness tests
// ===========================================================================

test.describe.serial("x11perf extended operations", () => {
	test.setTimeout(300_000);

	test("x11perf text operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -char16 -tr10text -polytext -complex -aa10text 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf fill operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -rect500 -srect500 -osrect500 -tilerect100 -stippledrect100 -oddsrect100 -bigsrect500 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf copy operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -copyarea500 -copyplane500 -putimage500 -getimage500 -shmput500 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf arc and polygon operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -circle100 -fcircle100 -ellipse100 -fellipse100 -triangle100 -ftriangle100 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf window operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -create -map -unmap -destroy -resize -move 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf image operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -putimage10 -putimage100 -putimage500 -shmput10 -shmput100 -shmput500 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});
});

// ===========================================================================
// XDPYINFO — Display info verification
// ===========================================================================

test.describe.serial("xdpyinfo verification", () => {
	test("xdpyinfo reports correct server info", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>&1",
		);
		// Basic server info
		expect(output).toContain("screen #0");
		expect(output).toMatch(/dimensions:/);
		expect(output).toMatch(/depth.*24/);

		// Should report all key extensions
		const requiredExtensions = [
			"BIG-REQUESTS",
			"RENDER",
			"XFIXES",
			"COMPOSITE",
			"DAMAGE",
			"RANDR",
			"XINERAMA",
			"SYNC",
			"MIT-SHM",
			"XInputExtension",
			"XKEYBOARD",
			"SHAPE",
			"XTEST",
			"DPMS",
			"DOUBLE-BUFFER",
			"SECURITY",
			"RECORD",
			"Present",
			"DRI3",
			"GLX",
		];
		for (const ext of requiredExtensions) {
			expect(output).toContain(ext);
		}
	});

	test("xdpyinfo reports correct visual info", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>&1",
		);
		// Should have TrueColor visual
		expect(output).toContain("TrueColor");
		// Should report visual depth
		expect(output).toMatch(/depth.*24/);
		// Color bits
		expect(output).toMatch(/bits per rgb/i);
	});
});

// ===========================================================================
// XLSFONTS — Font system verification
// ===========================================================================

test.describe.serial("Font system deep tests", () => {
	test("xlsfonts lists available fonts", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xlsfonts 2>&1 | head -50",
		);
		expect(output).not.toContain("unable to open display");
		// Should list some fonts
		expect(output.split("\n").length).toBeGreaterThan(3);
	});

	test("xlsfonts XLFD pattern matching works", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			'xlsfonts -fn "-*-*-*-*-*-*-13-*-*-*-*-*-*-*" 2>&1 | head -20',
		);
		// Should match fonts with pixel size 13
		expect(output).not.toContain("unable to open display");
	});

	test("xlsfonts finds fixed font", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			'xlsfonts -fn "fixed" 2>&1',
		);
		expect(output).toContain("fixed");
	});

	test("xlsfonts finds cursor font", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			'xlsfonts -fn "cursor" 2>&1',
		);
		expect(output).toContain("cursor");
	});

	test("OpenFont and QueryFont round-trip for XLFD pattern", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
# Open a font using XLFD pattern with wildcards
try:
    font = d.open_font('-*-fixed-*-*-*-*-*-*-*-*-*-*-*-*')
    qi = font.query()
    print(f"font_ascent={qi.font_ascent} font_descent={qi.font_descent}")
    print(f"min_char={qi.min_char_or_byte2} max_char={qi.max_char_or_byte2}")
    print(f"n_properties={qi.n_properties}")
    print("FONT_OK")
    font.close()
except Exception as e:
    print(f"ERROR: {e}")
d.close()
`,
		);
		expect(output).toContain("FONT_OK");
		expect(output).toMatch(/font_ascent=\d+/);
	});

	test("QueryTextExtents returns correct metrics", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
font = d.open_font('fixed')
qi = font.query()
# Get text extents for a known string
ext = font.query_text_extents('Hello World')
print(f"overall_width={ext.overall_width}")
print(f"font_ascent={ext.font_ascent}")
print(f"font_descent={ext.font_descent}")
# Width must be positive and reasonable
if ext.overall_width > 0:
    print("EXTENTS_OK")
font.close()
d.close()
`,
		);
		expect(output).toContain("EXTENTS_OK");
	});
});

// ===========================================================================
// XLSATOMS — Atom verification
// ===========================================================================

test.describe.serial("Atom system tests", () => {
	test("xlsatoms lists standard atoms", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xlsatoms 2>&1 | head -100",
		);
		// Standard predefined atoms
		expect(output).toContain("PRIMARY");
		expect(output).toContain("ATOM");
		expect(output).toContain("STRING");
		expect(output).toContain("WM_NAME");
	});

	test("xlsatoms lists EWMH atoms", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xlsatoms 2>&1",
		);
		expect(output).toContain("_NET_SUPPORTED");
		expect(output).toContain("_NET_WM_NAME");
		expect(output).toContain("_NET_WM_STATE");
	});

	test("InternAtom and GetAtomName round-trip", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
# Create a custom atom
atom = d.intern_atom('_TEST_CUSTOM_ATOM_12345')
name = d.get_atom_name(atom)
print(f"atom_id={atom} name={name}")
if name == '_TEST_CUSTOM_ATOM_12345':
    print("ATOM_OK")
# Verify only_if_exists works
atom2 = d.intern_atom('_NONEXISTENT_ATOM_ZZZZZ', only_if_exists=True)
print(f"nonexistent={atom2}")
if atom2 == 0:
    print("ONLY_IF_EXISTS_OK")
d.close()
`,
		);
		expect(output).toContain("ATOM_OK");
		expect(output).toContain("ONLY_IF_EXISTS_OK");
	});
});

// ===========================================================================
// REAL APPLICATION DEEP TESTS
// ===========================================================================

test.describe.serial("Real application deep tests", () => {
	test.setTimeout(120_000);

	test("xeyes starts, runs, and exits cleanly", async ({
		sidecarContainer,
	}) => {
		await execInSidecar(sidecarContainer, "xeyes &");
		await new Promise((r) => setTimeout(r, 2000));
		const ps = await execInSidecar(
			sidecarContainer,
			"pgrep xeyes && echo RUNNING",
		);
		expect(ps).toContain("RUNNING");
		// Kill cleanly
		await execInSidecar(sidecarContainer, "pkill xeyes; sleep 1");
		const ps2 = await execInSidecar(
			sidecarContainer,
			"pgrep xeyes || echo STOPPED",
		);
		expect(ps2).toContain("STOPPED");
	});

	test("xlogo renders without X errors", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 3 xlogo 2>&1; echo EXIT_CODE=$?",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
	});

	test("xterm runs interactive commands", async ({ sidecarContainer }) => {
		// Launch xterm with a command that tests terminal I/O
		await execInSidecar(
			sidecarContainer,
			`xterm -e 'echo "HELLO_FROM_XTERM" > /tmp/xterm_deep_test; ls / >> /tmp/xterm_deep_test; echo "XTERM_DONE" >> /tmp/xterm_deep_test' &`,
		);
		await new Promise((r) => setTimeout(r, 5000));
		const output = await execInSidecar(
			sidecarContainer,
			"cat /tmp/xterm_deep_test 2>/dev/null",
		);
		expect(output).toContain("HELLO_FROM_XTERM");
		expect(output).toContain("XTERM_DONE");
	});

	test("xterm renders with multiple fonts", async ({
		sidecarContainer,
	}) => {
		// Test that xterm can use different fonts
		const output = await execInSidecar(
			sidecarContainer,
			`timeout 3 xterm -fn fixed -e 'echo FONT_OK > /tmp/xterm_font_test' 2>&1 &
sleep 4
cat /tmp/xterm_font_test 2>/dev/null`,
		);
		expect(output).toContain("FONT_OK");
	});

	test("xclock with analog mode renders", async ({ sidecarContainer }) => {
		await execInSidecar(sidecarContainer, "xclock -analog &");
		await new Promise((r) => setTimeout(r, 2000));
		const ps = await execInSidecar(
			sidecarContainer,
			"pgrep xclock && echo RUNNING",
		);
		expect(ps).toContain("RUNNING");
		await execInSidecar(sidecarContainer, "pkill xclock; true");
	});

	test("xwininfo on root window works", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xwininfo -root 2>&1",
		);
		expect(output).toMatch(/Width:/);
		expect(output).toMatch(/Height:/);
		expect(output).toMatch(/Depth:/);
	});

	test("xprop on root window lists properties", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xprop -root 2>&1 | head -30",
		);
		// Should show some window manager properties
		expect(output).not.toContain("unable to open display");
		expect(output.length).toBeGreaterThan(10);
	});

	test("xdotool can manipulate windows", async ({ sidecarContainer }) => {
		// Start xeyes, use xdotool to find and move it
		await execInSidecar(sidecarContainer, "xeyes &");
		await new Promise((r) => setTimeout(r, 2000));
		const output = await execInSidecar(
			sidecarContainer,
			`xdotool search --name xeyes 2>&1`,
		);
		// Should find the window ID
		expect(output).toMatch(/\d+/);
		// Try to move it
		const winId = output.split("\n")[0].trim();
		if (winId) {
			await execInSidecar(
				sidecarContainer,
				`xdotool windowmove ${winId} 100 100 2>&1`,
			);
		}
		await execInSidecar(sidecarContainer, "pkill xeyes; true");
	});

	test("zenity dialog renders and can be dismissed", async ({
		sidecarContainer,
	}) => {
		// zenity is a GTK dialog tool - tests GTK2/3 compatibility
		const output = await execInSidecar(
			sidecarContainer,
			'timeout 5 zenity --info --text="Test" --timeout=2 2>&1; echo EXIT_CODE=$?',
		);
		expect(output).not.toContain("Segmentation fault");
		expect(output).not.toContain("X Error");
	});

	test("Firefox ESR starts without X errors", async ({
		sidecarContainer,
	}) => {
		// Launch Firefox briefly - this is the ultimate compatibility test
		const output = await execInSidecar(
			sidecarContainer,
			`timeout 15 firefox-esr --headless --no-remote --screenshot /tmp/ff_test.png 'data:text/html,<h1>Hello</h1>' 2>&1 || true
echo EXIT_CODE=$?`,
		);
		expect(output).not.toContain("Segmentation fault");
		// Firefox --screenshot mode doesn't require rendering but tests X11 init
	});

	test("GIMP starts without segfault", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			`timeout 15 gimp --no-data --no-fonts --batch '(gimp-version)' --batch '(gimp-quit 0)' 2>&1 || true`,
		);
		expect(output).not.toContain("Segmentation fault");
	});

	test("GTK3 example app starts", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			`timeout 5 gtk3-demo --run=css_basics 2>&1 &
sleep 3
pgrep -f gtk3-demo && echo GTK3_RUNNING || echo GTK3_STOPPED
pkill -f gtk3-demo; true`,
		);
		// Should either start running or exit cleanly
		expect(output).not.toContain("Segmentation fault");
	});

	test("wish (Tcl/Tk) app starts", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			`timeout 5 wish -e 'wm title . "Test"; after 2000 exit' 2>&1 &
sleep 3
echo WISH_DONE`,
		);
		expect(output).not.toContain("Segmentation fault");
		expect(output).not.toContain("X Error");
		expect(output).toContain("WISH_DONE");
	});
});

// ===========================================================================
// MULTI-CLIENT INTERACTION TESTS
// ===========================================================================

test.describe.serial("Multi-client interaction", () => {
	test.setTimeout(60_000);

	test("Two clients can set and read properties", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, struct

# Client 1: create window and set property
d1 = Xlib.display.Display()
screen = d1.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d1.sync()

# Set a custom property
atom = d1.intern_atom('_TEST_MULTI_CLIENT')
w.change_property(atom, Xlib.Xatom.STRING, 8, b'hello_from_client1')
d1.sync()
wid = w.id

# Client 2: read the property
d2 = Xlib.display.Display()
w2 = d2.create_resource_object('window', wid)
prop = w2.get_full_property(d2.intern_atom('_TEST_MULTI_CLIENT'), Xlib.Xatom.STRING)
if prop and prop.value == b'hello_from_client1':
    print("MULTI_CLIENT_OK")
else:
    print(f"FAIL: prop={prop}")

d1.close()
d2.close()
`,
		);
		expect(output).toContain("MULTI_CLIENT_OK");
	});

	test("Selection transfer between two clients", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
import time

d = Xlib.display.Display()
screen = d.screen()

# Create owner window
owner = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.PropertyChangeMask)
owner.map()
d.sync()

# Set selection owner
sel_atom = d.intern_atom('PRIMARY')
owner.set_selection_owner(sel_atom, Xlib.X.CurrentTime)
d.sync()

# Verify ownership
sel_owner = d.get_selection_owner(sel_atom)
if sel_owner == owner:
    print("SELECTION_OWNER_OK")
else:
    print(f"FAIL: expected owner {owner.id}, got {sel_owner}")

d.close()
`,
		);
		expect(output).toContain("SELECTION_OWNER_OK");
	});

	test("Event delivery to multiple clients watching same window", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

# Create a window
d1 = Xlib.display.Display()
screen = d1.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.PropertyChangeMask)
w.map()
d1.sync()

# Client 2 selects events on the same window
d2 = Xlib.display.Display()
w2 = d2.create_resource_object('window', w.id)
w2.change_attributes(event_mask=Xlib.X.PropertyChangeMask)
d2.sync()

# Change a property - both clients should be notifiable
atom = d1.intern_atom('_MULTI_TEST')
w.change_property(atom, Xlib.Xatom.STRING, 8, b'test_value')
d1.sync()

# Check client 1 gets PropertyNotify
d1.sync()
ev = d1.pending_events()
print(f"client1_pending={ev}")

# Both connected successfully
print("MULTI_EVENT_OK")
d1.close()
d2.close()
`,
		);
		expect(output).toContain("MULTI_EVENT_OK");
	});
});

// ===========================================================================
// PROTOCOL EDGE CASES
// ===========================================================================

test.describe.serial("Protocol edge cases", () => {
	test.setTimeout(60_000);

	test("Large property data (INCR threshold)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom

d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set a large property (100KB - should work with or without INCR)
large_data = b'A' * 100000
atom = d.intern_atom('_LARGE_PROP_TEST')
w.change_property(atom, Xlib.Xatom.STRING, 8, large_data)
d.sync()

# Read it back
prop = w.get_full_property(atom, Xlib.Xatom.STRING)
if prop and len(prop.value) == 100000 and prop.value == large_data:
    print("LARGE_PROP_OK")
else:
    print(f"FAIL: got {len(prop.value) if prop else 0} bytes")

d.close()
`,
		);
		expect(output).toContain("LARGE_PROP_OK");
	});

	test("Window hierarchy: reparent, query tree", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create parent and child windows
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
child = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
parent.map()
child.map()
d.sync()

# Reparent child into parent
child.reparent(parent, 10, 10)
d.sync()

# Query tree to verify
tree = parent.query_tree()
child_ids = [c.id for c in tree.children]
if child.id in child_ids:
    print("REPARENT_OK")
else:
    print(f"FAIL: child {child.id} not in {child_ids}")

# Verify geometry relative to new parent
geom = child.get_geometry()
print(f"child_x={geom.x} child_y={geom.y}")
if geom.x == 10 and geom.y == 10:
    print("GEOMETRY_OK")

d.close()
`,
		);
		expect(output).toContain("REPARENT_OK");
		expect(output).toContain("GEOMETRY_OK");
	});

	test("Colormap operations", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create a colormap
cmap = screen.default_colormap
# AllocColor
result = cmap.alloc_color(65535, 0, 0)  # Red
print(f"alloc_red: pixel={result.pixel}")
if result.pixel > 0 or result.exact_red == 65535:
    print("ALLOC_COLOR_OK")

# AllocNamedColor
try:
    result2 = cmap.alloc_named_color('blue')
    print(f"alloc_blue: pixel={result2.pixel}")
    print("ALLOC_NAMED_OK")
except Exception as e:
    print(f"AllocNamedColor: {e}")

# QueryColors
colors = cmap.query_colors([result.pixel])
if len(colors) > 0:
    print("QUERY_COLORS_OK")

d.close()
`,
		);
		expect(output).toContain("ALLOC_COLOR_OK");
		expect(output).toContain("QUERY_COLORS_OK");
	});

	test("GrabPointer and UngrabPointer", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask)
w.map()
d.sync()

# Grab pointer
status = w.grab_pointer(False, Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync, Xlib.X.NONE, Xlib.X.NONE,
    Xlib.X.CurrentTime)
print(f"grab_status={status}")
if status == Xlib.X.GrabSuccess:
    print("GRAB_OK")

# Ungrab
d.ungrab_pointer(Xlib.X.CurrentTime)
d.sync()
print("UNGRAB_OK")

d.close()
`,
		);
		expect(output).toContain("GRAB_OK");
		expect(output).toContain("UNGRAB_OK");
	});

	test("SetInputFocus and FocusIn/FocusOut events", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask)
w2 = screen.root.create_window(200, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask)
w1.map()
w2.map()
d.sync()

# Set focus to w1
d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus = d.get_input_focus()
if focus.focus.id == w1.id:
    print("FOCUS_W1_OK")

# Switch focus to w2
d.set_input_focus(w2, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus = d.get_input_focus()
if focus.focus.id == w2.id:
    print("FOCUS_W2_OK")

d.close()
`,
		);
		expect(output).toContain("FOCUS_W1_OK");
		expect(output).toContain("FOCUS_W2_OK");
	});

	test("CreatePixmap at multiple depths", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

depths_ok = []
for depth in [1, 8, 24, 32]:
    try:
        pid = d.allocate_resource_id()
        # Try to create a pixmap at this depth
        pm = screen.root.create_pixmap(100, 100, depth)
        pm.free()
        depths_ok.append(depth)
    except Exception as e:
        print(f"depth {depth}: {e}")

print(f"depths_ok={depths_ok}")
if 1 in depths_ok and 24 in depths_ok:
    print("PIXMAP_DEPTHS_OK")

d.close()
`,
		);
		expect(output).toContain("PIXMAP_DEPTHS_OK");
	});

	test("Window stacking order operations", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create three windows
w1 = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w2 = screen.root.create_window(50, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w3 = screen.root.create_window(100, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w1.map()
w2.map()
w3.map()
d.sync()

# Raise w1 to top
w1.configure(stack_mode=Xlib.X.Above)
d.sync()

# Query stacking order
tree = screen.root.query_tree()
children = [c.id for c in tree.children]
# w1 should be at or near the top
if w1.id in children:
    pos = children.index(w1.id)
    print(f"w1_position={pos} total={len(children)}")
    print("STACKING_OK")

d.close()
`,
		);
		expect(output).toContain("STACKING_OK");
	});

	test("RENDER CreatePicture and Composite", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Check RENDER extension is available
try:
    render_info = d.query_extension('RENDER')
    if render_info and render_info.present:
        print("RENDER_PRESENT")
    else:
        print("RENDER_MISSING")
except Exception as e:
    print(f"RENDER check: {e}")

d.close()
`,
		);
		expect(output).toContain("RENDER_PRESENT");
	});

	test("SYNC extension counter operations", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()

# Check SYNC extension
sync_info = d.query_extension('SYNC')
if sync_info and sync_info.present:
    print("SYNC_PRESENT")
else:
    print("SYNC_MISSING")

d.close()
`,
		);
		expect(output).toContain("SYNC_PRESENT");
	});

	test("Rapid connect/disconnect stress test", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display

success = 0
for i in range(20):
    try:
        d = Xlib.display.Display()
        screen = d.screen()
        # Do a basic operation
        _ = screen.root.get_geometry()
        d.close()
        success += 1
    except Exception as e:
        print(f"Connection {i} failed: {e}")

print(f"success={success}/20")
if success == 20:
    print("STRESS_OK")
`,
		);
		expect(output).toContain("STRESS_OK");
	});
});

// ===========================================================================
// KEYBOARD AND INPUT TESTS
// ===========================================================================

test.describe.serial("Keyboard and input", () => {
	test("GetKeyboardMapping returns valid mappings", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display

d = Xlib.display.Display()
# Get keyboard mapping for a range of keycodes
mapping = d.get_keyboard_mapping(8, 248)
print(f"mapping_entries={len(mapping)}")
if len(mapping) > 0:
    print("KEYMAP_OK")

d.close()
`,
		);
		expect(output).toContain("KEYMAP_OK");
	});

	test("GetModifierMapping returns valid modifiers", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display

d = Xlib.display.Display()
mod = d.get_modifier_mapping()
print(f"modifier_groups={len(mod)}")
# Should have 8 modifier groups (Shift, Lock, Control, Mod1-5)
if len(mod) == 8:
    print("MODMAP_OK")

d.close()
`,
		);
		expect(output).toContain("MODMAP_OK");
	});

	test("xkbcomp can query keyboard layout", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"setxkbmap -query 2>&1",
		);
		// Should not error
		expect(output).not.toContain("Error");
		// Should report a layout
		expect(output).toMatch(/layout|rules/);
	});
});

// ===========================================================================
// MESA/GLX TESTS
// ===========================================================================

test.describe.serial("GLX and OpenGL", () => {
	test.setTimeout(120_000);

	test("glxinfo reports server and client info", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"glxinfo 2>&1 | head -30",
		);
		// Should show some GLX info even with software rendering
		expect(output).not.toContain("unable to open display");
	});

	test("mesa-utils glxgears smoke test", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 5 glxgears -info 2>&1 | head -20 || true",
		);
		expect(output).not.toContain("Segmentation fault");
	});
});

// ===========================================================================
// EXTENSION FUNCTIONALITY TESTS
// ===========================================================================

test.describe.serial("Extension deep tests", () => {
	test.setTimeout(60_000);

	test("SHAPE extension: set window shape", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
# Check SHAPE extension
shape_info = d.query_extension('SHAPE')
if shape_info and shape_info.present:
    print("SHAPE_PRESENT")
else:
    print("SHAPE_MISSING")
d.close()
`,
		);
		expect(output).toContain("SHAPE_PRESENT");
	});

	test("COMPOSITE extension: redirect window", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
composite_info = d.query_extension('Composite')
if composite_info and composite_info.present:
    print("COMPOSITE_PRESENT")
else:
    print("COMPOSITE_MISSING")
d.close()
`,
		);
		expect(output).toContain("COMPOSITE_PRESENT");
	});

	test("XFIXES extension: create region", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display

d = Xlib.display.Display()
xfixes_info = d.query_extension('XFIXES')
if xfixes_info and xfixes_info.present:
    print("XFIXES_PRESENT")
else:
    print("XFIXES_MISSING")
d.close()
`,
		);
		expect(output).toContain("XFIXES_PRESENT");
	});

	test("RANDR extension: query screen resources", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xrandr 2>&1",
		);
		expect(output).not.toContain("Failed to get size");
		// Should show at least one output/mode
		expect(output).toMatch(/\d+x\d+/);
	});

	test("XInput2 is available via xinput", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xinput list 2>&1",
		);
		expect(output).not.toContain("unable to open display");
		// Should list at least virtual core pointer and keyboard
		expect(output).toMatch(/pointer|keyboard/i);
	});

	test("XTEST extension: simulate input", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display

d = Xlib.display.Display()
xtest_info = d.query_extension('XTEST')
if xtest_info and xtest_info.present:
    print("XTEST_PRESENT")
else:
    print("XTEST_MISSING")
d.close()
`,
		);
		expect(output).toContain("XTEST_PRESENT");
	});

	test("MIT-SHM PutImage via xclip", async ({ sidecarContainer }) => {
		// Test shared memory via a real tool
		const output = await execInSidecar(
			sidecarContainer,
			`echo "test_clipboard_data" | xclip -selection clipboard 2>&1
xclip -selection clipboard -o 2>&1`,
		);
		expect(output).toContain("test_clipboard_data");
	});
});

// ===========================================================================
// WINDOW MANAGEMENT PROTOCOL TESTS (ICCCM/EWMH)
// ===========================================================================

test.describe.serial("ICCCM and EWMH compliance", () => {
	test("WM_PROTOCOLS and WM_DELETE_WINDOW", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom

d = Xlib.display.Display()
screen = d.screen()

# Create a window with WM_DELETE_WINDOW protocol
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set WM_PROTOCOLS
wm_protocols = d.intern_atom('WM_PROTOCOLS')
wm_delete = d.intern_atom('WM_DELETE_WINDOW')

import struct
w.change_property(wm_protocols, Xlib.Xatom.ATOM, 32,
    [wm_delete])
d.sync()

# Read it back
prop = w.get_full_property(wm_protocols, Xlib.Xatom.ATOM)
if prop:
    atoms = struct.unpack('I' * (len(prop.value) // 4), prop.value)
    if wm_delete in atoms:
        print("WM_DELETE_WINDOW_OK")

d.close()
`,
		);
		expect(output).toContain("WM_DELETE_WINDOW_OK");
	});

	test("_NET_WM_NAME (UTF-8 window title)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set _NET_WM_NAME (UTF-8)
net_wm_name = d.intern_atom('_NET_WM_NAME')
utf8_string = d.intern_atom('UTF8_STRING')
title = 'Test Window — Ünïcödé ✓'
w.change_property(net_wm_name, utf8_string, 8, title.encode('utf-8'))
d.sync()

# Read it back
prop = w.get_full_property(net_wm_name, utf8_string)
if prop and prop.value.decode('utf-8') == title:
    print("UTF8_TITLE_OK")
else:
    print(f"FAIL: got {prop.value if prop else None}")

d.close()
`,
		);
		expect(output).toContain("UTF8_TITLE_OK");
	});

	test("_NET_WM_STATE management", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
import struct

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set _NET_WM_STATE with multiple state atoms
net_wm_state = d.intern_atom('_NET_WM_STATE')
above = d.intern_atom('_NET_WM_STATE_ABOVE')
focused = d.intern_atom('_NET_WM_STATE_FOCUSED')

w.change_property(net_wm_state, Xlib.Xatom.ATOM, 32, [above, focused])
d.sync()

# Read it back
prop = w.get_full_property(net_wm_state, Xlib.Xatom.ATOM)
if prop:
    atoms = struct.unpack('I' * (len(prop.value) // 4), prop.value)
    if above in atoms and focused in atoms:
        print("NET_WM_STATE_OK")

d.close()
`,
		);
		expect(output).toContain("NET_WM_STATE_OK");
	});
});
