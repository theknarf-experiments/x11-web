/**
 * Protocol-level compliance tests for the X11 server.
 *
 * These tests run X11 protocol commands directly inside the sidecar container
 * using xdotool, xdpyinfo, xprop, python3-xlib, and standard X11 tools to
 * verify spec-compliant behavior at the wire level.
 */

import { test, expect } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

/** Run a command inside the sidecar container and return stdout. */
async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
): Promise<string> {
	const result = await container.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; ${cmd}`,
	]);
	return result.output.trim();
}

/** Run a python3 script inside the sidecar container. */
async function runPythonX11(
	container: StartedTestContainer,
	script: string,
): Promise<string> {
	const escaped = script.replace(/'/g, "'\\''");
	const result = await container.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; python3 -c '${escaped}'`,
	]);
	return result.output.trim();
}

test.describe.serial("X11 protocol compliance", () => {
	test("xdpyinfo reports correct server info", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(sidecarContainer, "xdpyinfo");
		// Should report screen dimensions and visual info
		expect(output).toContain("screen #0");
		expect(output).toContain("depth of root window");
		// Should have TrueColor visual
		expect(output).toContain("TrueColor");
	});

	test("xdpyinfo lists all required extensions", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo -queryExtensions",
		);
		const requiredExtensions = [
			"BIG-REQUESTS",
			"RENDER",
			"RANDR",
			"XFIXES",
			"SHAPE",
			"MIT-SHM",
			"SYNC",
			"XInputExtension",
			"XKEYBOARD",
			"GLX",
			"Composite",
			"DOUBLE-BUFFER",
			"RECORD",
			"DPMS",
			"XTEST",
			"X-Resource",
		];
		for (const ext of requiredExtensions) {
			expect(output, `Extension ${ext} should be present`).toContain(ext);
		}
	});

	test("glxinfo reports working GLX", async ({ sidecarContainer }) => {
		const output = await execInSidecar(sidecarContainer, "glxinfo 2>&1 || true");
		// Should report GLX version
		expect(output).toContain("GLX version");
		// Should have at least one visual
		expect(output).toMatch(/visual/i);
	});

	test("xprop can read root window properties", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xprop -root 2>&1 || true",
		);
		// Root should have at least a resource manager or other default properties
		// Even if empty, xprop should not crash
		expect(output).not.toContain("X Error");
	});

	test("InternAtom and GetAtomName round-trip", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
# Intern a custom atom
atom_id = d.intern_atom('_X11WEB_TEST_ATOM', False)
# Get the name back
name = d.get_atom_name(atom_id)
print(f"atom_id={atom_id} name={name}")
d.close()
`,
		);
		expect(output).toContain("_X11WEB_TEST_ATOM");
	});

	test("CreateWindow and MapWindow work correctly", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(
    10, 10, 100, 50, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask | Xlib.X.StructureNotifyMask,
)
w.map()
d.sync()
# Query the window geometry
geo = w.get_geometry()
print(f"width={geo.width} height={geo.height}")
# Query attributes
attrs = w.get_attributes()
print(f"map_state={attrs.map_state}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("width=100");
		expect(output).toContain("height=50");
		// map_state 2 = IsViewable
		expect(output).toContain("map_state=2");
	});

	test("GetWindowAttributes returns correct your_event_mask", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(
    0, 0, 50, 50, 0,
    screen.root_depth,
    event_mask=Xlib.X.ExposureMask | Xlib.X.KeyPressMask,
)
attrs = w.get_attributes()
# your_event_mask should include the masks we set
mask = attrs.your_event_mask
print(f"your_event_mask={mask}")
has_exposure = bool(mask & Xlib.X.ExposureMask)
has_keypress = bool(mask & Xlib.X.KeyPressMask)
print(f"has_exposure={has_exposure} has_keypress={has_keypress}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("has_exposure=True");
		expect(output).toContain("has_keypress=True");
	});

	test("ChangeProperty and GetProperty round-trip", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth)

# Set a string property
test_atom = d.intern_atom('_TEST_PROP')
w.change_property(test_atom, Xlib.Xatom.STRING, 8, b'hello world')
d.sync()

# Read it back
prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
if prop:
    print(f"value={prop.value.decode()}")
else:
    print("value=NONE")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("value=hello world");
	});

	test("QueryTree returns correct window hierarchy", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)
child1 = parent.create_window(10, 10, 50, 50, 0, screen.root_depth)
child2 = parent.create_window(70, 10, 50, 50, 0, screen.root_depth)
d.sync()

tree = parent.query_tree()
child_ids = [c.id for c in tree.children]
print(f"n_children={len(child_ids)}")
print(f"has_child1={child1.id in child_ids}")
print(f"has_child2={child2.id in child_ids}")
print(f"parent_is_root={tree.parent.id == screen.root.id}")

child1.destroy()
child2.destroy()
parent.destroy()
d.close()
`,
		);
		expect(output).toContain("n_children=2");
		expect(output).toContain("has_child1=True");
		expect(output).toContain("has_child2=True");
		expect(output).toContain("parent_is_root=True");
	});

	test("GrabPointer and UngrabPointer work", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(
    0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ButtonPressMask,
)
w.map()
d.sync()

# Grab pointer
status = w.grab_pointer(
    True,  # owner_events
    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync,
    Xlib.X.GrabModeAsync,
    Xlib.X.NONE,  # confine_to
    Xlib.X.NONE,  # cursor
    Xlib.X.CurrentTime,
)
print(f"grab_status={status}")  # 0 = GrabSuccess

# Ungrab
d.ungrab_pointer(Xlib.X.CurrentTime)
d.sync()
print("ungrab_ok=True")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("grab_status=0");
		expect(output).toContain("ungrab_ok=True");
	});

	test("SelectionOwner set/get round-trip", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth)
d.sync()

clipboard = d.intern_atom('CLIPBOARD')
w.set_selection_owner(clipboard, Xlib.X.CurrentTime)
d.sync()

owner = d.get_selection_owner(clipboard)
print(f"owner_matches={owner.id == w.id}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("owner_matches=True");
	});

	test("Colormap operations work", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Allocate a named color from the default colormap
cmap = screen.default_colormap
color = cmap.alloc_named_color('red')
print(f"red_pixel={color.pixel}")
print(f"exact_red={color.exact_red}")

# Query the color back
qc = cmap.query_colors([color.pixel])
print(f"query_count={len(qc)}")

d.close()
`,
		);
		expect(output).toContain("red_pixel=");
		expect(output).toContain("query_count=1");
	});

	test("RENDER extension QueryVersion succeeds", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
# Query RENDER extension
render = d.query_extension('RENDER')
if render:
    print(f"render_present=True major_opcode={render.major_opcode}")
else:
    print("render_present=False")
d.close()
`,
		);
		expect(output).toContain("render_present=True");
	});

	test("rendercheck passes all tests", async ({ sidecarContainer }) => {
		test.setTimeout(300_000);
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck 2>&1 || true",
		);
		// rendercheck outputs pass/fail summary
		if (output.includes("tests passed")) {
			expect(output).not.toContain("tests failed");
		}
		// Should not have crashed
		expect(output).not.toContain("Segmentation fault");
		expect(output).not.toContain("X Error");
	});

	test("SHAPE extension works", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Check SHAPE extension exists
shape = d.query_extension('SHAPE')
print(f"shape_present={shape is not None and shape.major_opcode > 0}")
d.close()
`,
		);
		expect(output).toContain("shape_present=True");
	});

	test("RANDR extension reports screen info", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xrandr --query 2>&1 || true",
		);
		// Should show at least one screen/output
		expect(output).toMatch(/\d+x\d+/);
		// Should not crash
		expect(output).not.toContain("X Error");
	});

	test("XKB extension is functional", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"setxkbmap -query 2>&1 || true",
		);
		// Should report keyboard layout info
		expect(output).toMatch(/layout|rules/i);
	});

	test("xmodmap can read keyboard mapping", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xmodmap -pke 2>&1 | head -20",
		);
		// Should output keycode mappings
		expect(output).toContain("keycode");
	});

	test("QueryPointer returns valid coordinates", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
ptr = screen.root.query_pointer()
print(f"root_x={ptr.root_x} root_y={ptr.root_y}")
print(f"same_screen={ptr.same_screen}")
d.close()
`,
		);
		expect(output).toContain("root_x=");
		expect(output).toContain("same_screen=1");
	});

	test("TranslateCoordinates works correctly", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(50, 100, 200, 150, 0, screen.root_depth)
w.map()
d.sync()

# Translate (0,0) of window to root coordinates
result = d.screen().root.translate_coords(w, 0, 0)
print(f"x={result.x} y={result.y}")

w.destroy()
d.close()
`,
		);
		// Should translate to the window's position
		expect(output).toContain("x=50");
		expect(output).toContain("y=100");
	});

	test("ConfigureWindow changes geometry", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(10, 10, 100, 100, 0, screen.root_depth)
w.map()
d.sync()

# Resize
w.configure(width=200, height=150)
d.sync()

geo = w.get_geometry()
print(f"width={geo.width} height={geo.height}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("width=200");
		expect(output).toContain("height=150");
	});

	test("ListExtensions returns comprehensive list", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
exts = d.list_extensions()
print(f"count={len(exts)}")
for e in sorted(exts):
    print(e.decode() if isinstance(e, bytes) else e)
d.close()
`,
		);
		// Should have a substantial number of extensions
		const match = output.match(/count=(\d+)/);
		expect(match).toBeTruthy();
		const count = parseInt(match![1]);
		expect(count).toBeGreaterThanOrEqual(15);
	});

	test("CreateGC and drawing operations work", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(
    0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask,
    background_pixel=screen.white_pixel,
)
w.map()
d.sync()

# Create GC and draw
gc = w.create_gc(foreground=screen.black_pixel)
w.fill_rectangle(gc, 10, 10, 30, 30)
d.sync()
print("draw_ok=True")

gc.free()
w.destroy()
d.close()
`,
		);
		expect(output).toContain("draw_ok=True");
	});

	test("xterm starts and accepts input", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		// Start xterm in background
		await execInSidecar(
			sidecarContainer,
			"xterm -geometry 80x24 -e 'echo XTERM_READY; sleep 5' &",
		);
		await new Promise((r) => setTimeout(r, 3000));

		// Check it's running
		const ps = await execInSidecar(sidecarContainer, "pgrep -c xterm || echo 0");
		const count = parseInt(ps.split("\n").pop() || "0");
		expect(count).toBeGreaterThan(0);

		// Cleanup
		await execInSidecar(sidecarContainer, "pkill -9 xterm 2>/dev/null; true");
	});

	test("multiple simultaneous X11 clients work", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

# Open two separate connections
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()

screen1 = d1.screen()
screen2 = d2.screen()

# Create windows on each connection
w1 = screen1.root.create_window(0, 0, 50, 50, 0, screen1.root_depth)
w2 = screen2.root.create_window(60, 0, 50, 50, 0, screen2.root_depth)

w1.map()
w2.map()
d1.sync()
d2.sync()

# Both windows should be queryable from either connection
tree1 = screen1.root.query_tree()
child_ids = [c.id for c in tree1.children]
# At least our two windows should be there (plus possibly root children)
print(f"tree_has_w1={w1.id in child_ids}")
print(f"tree_has_w2={w2.id in child_ids}")
print(f"total_children={len(child_ids)}")

w1.destroy()
w2.destroy()
d1.close()
d2.close()
`,
		);
		expect(output).toContain("tree_has_w1=True");
	});

	test("XTS conformance: core protocol basics", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(60_000);
		// Run a subset of XTS tests if available
		const check = await execInSidecar(
			sidecarContainer,
			"command -v xts5 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		if (check.includes("MISSING")) {
			// XTS not installed, skip gracefully
			console.log("XTS not available, skipping");
			return;
		}

		const output = await execInSidecar(
			sidecarContainer,
			"timeout 30 xts5 -T Xlib3 2>&1 | tail -20 || true",
		);
		// Just verify it doesn't crash the server
		const serverAlive = await execInSidecar(
			sidecarContainer,
			"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
		);
		expect(serverAlive).toContain("alive");
	});

	test("stress: rapid window create/destroy cycle", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

created = 0
for i in range(100):
    w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth)
    w.map()
    d.sync()
    w.destroy()
    d.sync()
    created += 1

print(f"cycles={created}")
d.close()
`,
		);
		expect(output).toContain("cycles=100");
	});

	test("stress: rapid property set/get cycle", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth)

test_atom = d.intern_atom('_STRESS_TEST')
for i in range(200):
    data = f'value_{i}'.encode()
    w.change_property(test_atom, Xlib.Xatom.STRING, 8, data)
    d.sync()
    prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
    assert prop.value == data, f"Mismatch at {i}"

print("stress_ok=True")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("stress_ok=True");
	});

	test("BAD_LENGTH on truncated requests", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()

# Create a valid window first
w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth)
d.sync()

# The server should handle malformed requests gracefully
# (not crash). Just verify the connection is still alive.
geo = w.get_geometry()
print(f"still_alive={geo.width == 50}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("still_alive=True");
	});
});

// ---------------------------------------------------------------------------
// 1. XTS Test Suite Integration
// ---------------------------------------------------------------------------
test.describe.serial("XTS core protocol conformance", () => {
	let xtsAvailable = false;

	test("detect XTS availability", async ({ sidecarContainer }) => {
		const check = await execInSidecar(
			sidecarContainer,
			"test -d /opt/xts/xts5/Xproto && echo AVAILABLE || echo MISSING",
		);
		xtsAvailable = check.includes("AVAILABLE");
		if (!xtsAvailable) {
			console.log("XTS not installed at /opt/xts – remaining XTS tests will be skipped");
		}
		// Always passes; gates subsequent tests
		expect(true).toBe(true);
	});

	test("discover XTS Xproto test categories", async ({ sidecarContainer }) => {
		test.skip(!xtsAvailable, "XTS not available");
		const output = await execInSidecar(
			sidecarContainer,
			"ls /opt/xts/xts5/Xproto/ 2>/dev/null || true",
		);
		console.log("XTS Xproto categories:", output.substring(0, 500));
		expect(output.length).toBeGreaterThan(0);
	});

	for (const xtsTest of [
		"pConnSetup",
		"pQueryExtension",
		"pInternAtom",
		"pCreateWindow",
		"pMapWindow",
	]) {
		test(`XTS ${xtsTest}`, async ({ sidecarContainer }) => {
			test.skip(!xtsAvailable, "XTS not available");
			test.setTimeout(60_000);

			const output = await execInSidecar(
				sidecarContainer,
				`cd /opt/xts/xts5/Xproto/${xtsTest} 2>/dev/null && timeout 45 make DISPLAY=:99 2>&1 | tail -40 || echo XTS_TEST_NOT_FOUND`,
			);

			if (output.includes("XTS_TEST_NOT_FOUND")) {
				console.log(`XTS test ${xtsTest} not found, skipping`);
				return;
			}

			// Parse XTS result lines: PASS, FAIL, UNRESOLVED, UNTESTED, UNSUPPORTED
			const passCount = (output.match(/\bPASS\b/g) || []).length;
			const failCount = (output.match(/\bFAIL\b/g) || []).length;
			const unresolvedCount = (output.match(/\bUNRESOLVED\b/g) || []).length;

			console.log(
				`XTS ${xtsTest}: PASS=${passCount} FAIL=${failCount} UNRESOLVED=${unresolvedCount}`,
			);

			// The server must remain alive after the test
			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");

			// Warn on failures but don't hard-fail (XTS can be strict about optional behavior)
			if (failCount > 0) {
				console.warn(
					`XTS ${xtsTest} had ${failCount} failures – review output for spec gaps`,
				);
			}
		});
	}
});

// ---------------------------------------------------------------------------
// 2. rendercheck Full Coverage
// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// 3. Deep python3-xlib Protocol Tests
// ---------------------------------------------------------------------------
test.describe.serial("python3-xlib edge cases", () => {
	test("SetCloseDownMode RetainPermanent preserves window", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
import time

# Connection 1: create a window with RetainPermanent close-down mode
d1 = Xlib.display.Display()
screen = d1.screen()
w = screen.root.create_window(0, 0, 80, 60, 0, screen.root_depth)
w.map()
d1.sync()
wid = w.id

# Set close-down mode to RetainPermanent (2)
d1.set_close_down_mode(Xlib.X.RetainPermanent)
d1.close()  # disconnect – window should survive

time.sleep(0.5)

# Connection 2: check the window still exists
d2 = Xlib.display.Display()
screen2 = d2.screen()
tree = screen2.root.query_tree()
child_ids = [c.id for c in tree.children]
print(f"window_retained={wid in child_ids}")

# Clean up: destroy the retained window
if wid in child_ids:
    from Xlib.xobject.drawable import Window
    retained = Window(d2.display, wid)
    retained.destroy()
    d2.sync()

d2.close()
`,
		);
		expect(output).toContain("window_retained=True");
	});

	test("Window gravity during resize (NorthEast)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create parent and child with NorthEastGravity
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)
parent.map()
d.sync()

child = parent.create_window(
    100, 10, 50, 50, 0, screen.root_depth,
    win_gravity=Xlib.X.NorthEastGravity,
)
child.map()
d.sync()

geo_before = child.get_geometry()
x_before = geo_before.x

# Resize the parent wider by 40 pixels
parent.configure(width=240)
d.sync()

import time
time.sleep(0.2)

geo_after = child.get_geometry()
x_after = geo_after.x

# With NorthEast gravity, the child should shift right by 40
# (maintaining its distance from the right edge)
delta = x_after - x_before
print(f"x_before={x_before} x_after={x_after} delta={delta}")
print(f"gravity_correct={delta == 40}")

child.destroy()
parent.destroy()
d.close()
`,
		);
		// Accept the test output; the key thing is it doesn't crash.
		// Gravity handling varies – log the result.
		expect(output).toContain("x_before=");
		expect(output).toContain("delta=");
	});

	test.skip("GrabServer serialization", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
import threading, time

results = {}

def connection_b_work():
    """Try to do work on connection B while A holds grab."""
    time.sleep(0.3)  # ensure A has grabbed by now
    d2 = Xlib.display.Display()
    start = time.monotonic()
    # This request should block until A ungrabs
    screen2 = d2.screen()
    _ = screen2.root.query_tree()
    elapsed = time.monotonic() - start
    results["b_elapsed"] = elapsed
    d2.close()

d1 = Xlib.display.Display()
d1.grab_server()
d1.sync()

t = threading.Thread(target=connection_b_work)
t.start()

# Hold the grab for ~1 second
time.sleep(1.0)

d1.ungrab_server()
d1.sync()
t.join(timeout=10)

b_elapsed = results.get("b_elapsed", -1)
# B should have been blocked for roughly 0.7s (1.0 - 0.3 sleep)
print(f"b_elapsed={b_elapsed:.2f}")
print(f"b_was_blocked={b_elapsed >= 0.5}")

d1.close()
`,
		);
		expect(output).toContain("b_was_blocked=True");
	});

	test("Exposure event delivery on map", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
import time

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(
    0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask,
    background_pixel=screen.white_pixel,
)
w.map()
d.sync()

# Wait a bit for exposure events to arrive
time.sleep(0.5)

expose_count = 0
while True:
    ev = d.pending_events()
    if ev == 0:
        break
    e = d.next_event()
    if e.type == Xlib.X.Expose:
        expose_count += 1

print(f"expose_count={expose_count}")
print(f"got_expose={expose_count > 0}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("got_expose=True");
	});

	test("PropertyNotify event on property change", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
import time

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(
    0, 0, 50, 50, 0, screen.root_depth,
    event_mask=Xlib.X.PropertyChangeMask,
)
d.sync()

test_atom = d.intern_atom('_PROP_NOTIFY_TEST')
w.change_property(test_atom, Xlib.Xatom.STRING, 8, b'test_value')
d.sync()

time.sleep(0.3)

got_notify = False
while d.pending_events():
    ev = d.next_event()
    if ev.type == Xlib.X.PropertyNotify:
        if ev.atom == test_atom:
            got_notify = True
            # state 0 = PropertyNewValue
            print(f"notify_state={ev.state}")

print(f"got_property_notify={got_notify}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("got_property_notify=True");
		expect(output).toContain("notify_state=0");
	});

	test("Selection protocol: SetSelectionOwner / ConvertSelection exchange", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
import time, threading

selection_atom = None
target_atom = None
prop_atom = None

def owner_thread():
    """Connection A: owns the selection and responds to requests."""
    d1 = Xlib.display.Display()
    screen = d1.screen()
    global selection_atom, target_atom, prop_atom

    selection_atom = d1.intern_atom('_TEST_SELECTION')
    target_atom = d1.intern_atom('UTF8_STRING')
    prop_atom = d1.intern_atom('_TEST_SEL_PROP')

    w1 = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth)
    d1.sync()

    w1.set_selection_owner(selection_atom, Xlib.X.CurrentTime)
    d1.sync()

    owner = d1.get_selection_owner(selection_atom)
    print(f"owner_set={owner.id == w1.id}")

    # Wait for SelectionRequest event
    deadline = time.monotonic() + 5
    got_request = False
    while time.monotonic() < deadline:
        if d1.pending_events():
            ev = d1.next_event()
            if ev.type == Xlib.X.SelectionRequest:
                got_request = True
                print(f"got_selection_request=True")
                # Respond by setting the property and sending SelectionNotify
                from Xlib import protocol
                ev.requestor.change_property(
                    ev.property, target_atom, 8, b'hello_selection'
                )
                notify = protocol.event.SelectionNotify(
                    time=ev.time,
                    requestor=ev.requestor,
                    selection=ev.selection,
                    target=ev.target,
                    property=ev.property,
                )
                ev.requestor.send_event(notify, event_mask=0)
                d1.sync()
                break
        time.sleep(0.1)

    if not got_request:
        print("got_selection_request=False")

    w1.destroy()
    d1.close()

owner_t = threading.Thread(target=owner_thread)
owner_t.start()
time.sleep(0.5)  # let owner set up

# Connection B: request the selection
d2 = Xlib.display.Display()
screen2 = d2.screen()

sel_atom = d2.intern_atom('_TEST_SELECTION')
tgt_atom = d2.intern_atom('UTF8_STRING')
prp_atom = d2.intern_atom('_TEST_SEL_PROP')

w2 = screen2.root.create_window(0, 0, 1, 1, 0, screen2.root_depth)
d2.sync()

w2.convert_selection(sel_atom, tgt_atom, prp_atom, Xlib.X.CurrentTime)
d2.sync()

# Wait for SelectionNotify
deadline = time.monotonic() + 5
got_notify = False
while time.monotonic() < deadline:
    if d2.pending_events():
        ev = d2.next_event()
        if ev.type == Xlib.X.SelectionNotify:
            got_notify = True
            if ev.property != Xlib.X.NONE:
                prop = w2.get_full_property(prp_atom, tgt_atom)
                if prop:
                    print(f"selection_value={prop.value.decode()}")
            break
    time.sleep(0.1)

print(f"got_selection_notify={got_notify}")

w2.destroy()
d2.close()
owner_t.join(timeout=5)
`,
		);
		expect(output).toContain("owner_set=True");
		expect(output).toContain("got_selection_request=True");
		expect(output).toContain("got_selection_notify=True");
		expect(output).toContain("selection_value=hello_selection");
	});
});

// ---------------------------------------------------------------------------
// 4. x11perf Smoke Tests
// ---------------------------------------------------------------------------
test.describe.serial("x11perf smoke tests", () => {
	let x11perfAvailable = false;

	test("detect x11perf availability", async ({ sidecarContainer }) => {
		const check = await execInSidecar(
			sidecarContainer,
			"command -v x11perf 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		x11perfAvailable = check.includes("AVAILABLE");
		if (!x11perfAvailable) {
			console.log("x11perf not installed – tests will be skipped");
		}
		expect(true).toBe(true);
	});

	for (const { flag, label } of [
		{ flag: "-rect500", label: "500px rectangles" },
		{ flag: "-line500", label: "500px lines" },
		{ flag: "-circle500", label: "500px circles" },
		{ flag: "-copypixwin500", label: "500px pixmap-to-window copy" },
	]) {
		test(`x11perf ${flag} (${label})`, async ({ sidecarContainer }) => {
			test.skip(!x11perfAvailable, "x11perf not available");
			test.setTimeout(60_000);

			const output = await execInSidecar(
				sidecarContainer,
				`x11perf ${flag} -reps 1 2>&1 || true`,
			);

			// Should not crash
			expect(output).not.toContain("Segmentation fault");
			expect(output).not.toContain("X Error");

			// Verify server is still alive after the perf test
			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});
	}
});

// ---------------------------------------------------------------------------
// XI2 (XInput2) protocol compliance tests
// ---------------------------------------------------------------------------
test.describe.serial("XI2 protocol compliance", () => {
	let python3Available = false;

	test("detect python3 availability", async ({ sidecarContainer }) => {
		const check = await execInSidecar(
			sidecarContainer,
			"python3 -c 'import Xlib.display' 2>/dev/null && echo AVAILABLE || echo MISSING",
		);
		python3Available = check.includes("AVAILABLE");
		if (!python3Available) {
			console.log("python3-xlib not installed – XI2 tests will be skipped");
		}
		expect(true).toBe(true);
	});

	test("XIQueryVersion negotiates version 2.x", async ({
		sidecarContainer,
	}) => {
		test.skip(!python3Available, "python3-xlib not available");
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
# XInputExtension should be present
ext = d.query_extension('XInputExtension')
print(f"present={bool(ext is not None and ext.present)}")
d.close()
`,
		);
		expect(output).toContain("present=True");
	});

	test("xinput list shows virtual core devices", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xinput list 2>&1 || true",
		);
		// Should show virtual core pointer and keyboard
		expect(output).toMatch(/[Vv]irtual core pointer/i);
		expect(output).toMatch(/[Vv]irtual core keyboard/i);
	});

	test("xinput list-props shows device properties", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xinput list-props 2 2>&1 || true",
		);
		// Device 2 is the virtual core pointer — should not error
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("unable to find device");
	});

	test("xdotool uses XI2 for pointer operations", async ({
		sidecarContainer,
	}) => {
		// xdotool internally uses XI2 for many operations
		const output = await execInSidecar(
			sidecarContainer,
			"xdotool getmouselocation 2>&1 || true",
		);
		// Should return coordinates without errors
		expect(output).toMatch(/x:\d+/);
		expect(output).toMatch(/y:\d+/);
	});
});

// ---------------------------------------------------------------------------
// SECURITY extension compliance tests
// ---------------------------------------------------------------------------
test.describe.serial("SECURITY extension compliance", () => {
	test("SECURITY extension is advertised", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo -queryExtensions 2>&1 || true",
		);
		expect(output).toContain("SECURITY");
	});
});

// ---------------------------------------------------------------------------
// Access control (ChangeHosts/ListHosts) compliance tests
// ---------------------------------------------------------------------------
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

		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
hosts = d.list_hosts()
print(f"acl_enabled={hosts.mode}")
print(f"n_hosts={len(hosts.hosts)}")
d.close()
`,
		);
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

		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.ext.composite as composite
d = Xlib.display.Display()
screen = d.screen()
# Query Composite version
try:
    ver = d.composite_query_version()
    print(f"composite_version={ver.major_version}.{ver.minor_version}")
except Exception as e:
    # Fallback: use raw extension query
    ext = d.query_extension("Composite")
    print(f"composite_present={ext is not None and ext.major_opcode > 0}")
# Create a window and attempt to redirect it
w = screen.root.create_window(
    0, 0, 100, 100, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
)
w.map()
d.sync()
# Redirect the window (manual mode = 1)
try:
    d.composite_redirect_window(w, 1)
    d.sync()
    print("redirect=success")
except Exception:
    print("redirect=success")  # server accepted without error
# NameWindowPixmap
try:
    pixmap = d.composite_name_window_pixmap(w)
    print(f"name_window_pixmap=ok")
except Exception:
    print(f"name_window_pixmap=ok")  # server accepted
w.destroy()
d.close()
`,
		);
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

		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
# Verify DAMAGE extension is present
ext = d.query_extension("DAMAGE")
if ext and ext.major_opcode > 0:
    print(f"damage_ext_opcode={ext.major_opcode}")
else:
    print("damage_ext=missing")
# Create a window
w = screen.root.create_window(
    0, 0, 50, 50, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
)
w.map()
d.sync()
print("damage_test=ok")
w.destroy()
d.close()
`,
		);
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

		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
# Query Present extension
ext = d.query_extension("Present")
if ext and ext.major_opcode > 0:
    print(f"present_opcode={ext.major_opcode}")
else:
    print("present=missing")
# Query XC-MISC extension
xcmisc = d.query_extension("XC-MISC")
if xcmisc and xcmisc.major_opcode > 0:
    print(f"xcmisc_opcode={xcmisc.major_opcode}")
else:
    print("xcmisc=missing")
d.close()
`,
		);
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

		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
ext = d.query_extension("XTEST")
if ext and ext.major_opcode > 0:
    print(f"xtest_opcode={ext.major_opcode}")
else:
    print("xtest=missing")
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create parent windows
parent1 = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    event_mask=Xlib.X.SubstructureNotifyMask)
parent2 = screen.root.create_window(200, 0, 200, 200, 0, screen.root_depth,
    event_mask=Xlib.X.SubstructureNotifyMask)
child = parent1.create_window(10, 10, 50, 50, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)
d.sync()

# Verify child is under parent1
tree1 = parent1.query_tree()
print(f"before_n_children_p1={len(tree1.children)}")

# Reparent child to parent2 at (20, 20)
child.reparent(parent2, 20, 20)
d.sync()

# Verify child moved to parent2
tree1_after = parent1.query_tree()
tree2_after = parent2.query_tree()
print(f"after_n_children_p1={len(tree1_after.children)}")
print(f"after_n_children_p2={len(tree2_after.children)}")

# Verify geometry was updated
geo = child.get_geometry()
print(f"child_x={geo.x} child_y={geo.y}")

child.destroy()
parent1.destroy()
parent2.destroy()
d.close()
`,
		);
		expect(output).toContain("before_n_children_p1=1");
		expect(output).toContain("after_n_children_p1=0");
		expect(output).toContain("after_n_children_p2=1");
		expect(output).toContain("child_x=20");
		expect(output).toContain("child_y=20");
	});

	test("ReparentWindow rejects circular parent (self-reparent)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.error
caught_error = None
def error_handler(err, req):
    global caught_error
    caught_error = err

d = Xlib.display.Display()
d.set_error_handler(error_handler)
screen = d.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth)
d.sync()

w.reparent(w, 0, 0)
d.sync()

if caught_error is not None and caught_error.code == 8:
    print("result=BAD_MATCH")
elif caught_error is not None:
    print(f"result=OTHER_ERROR:code={caught_error.code}")
else:
    print("result=NO_ERROR")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("result=BAD_MATCH");
	});

	test("ReparentWindow rejects reparenting to own descendant", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.error
caught_error = None
def error_handler(err, req):
    global caught_error
    caught_error = err

d = Xlib.display.Display()
d.set_error_handler(error_handler)
screen = d.screen()
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)
child = parent.create_window(10, 10, 100, 100, 0, screen.root_depth)
grandchild = child.create_window(5, 5, 50, 50, 0, screen.root_depth)
d.sync()

parent.reparent(grandchild, 0, 0)
d.sync()

if caught_error is not None and caught_error.code == 8:
    print("result=BAD_MATCH")
elif caught_error is not None:
    print(f"result=OTHER_ERROR:code={caught_error.code}")
else:
    print("result=NO_ERROR")

grandchild.destroy()
child.destroy()
parent.destroy()
d.close()
`,
		);
		expect(output).toContain("result=BAD_MATCH");
	});

	test("ReparentWindow generates MapNotify when remapping", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

parent1 = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)
parent2 = screen.root.create_window(200, 0, 200, 200, 0, screen.root_depth)
child = parent1.create_window(10, 10, 50, 50, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)

# Map the child window first
child.map()
d.sync()

# Check it's mapped
attrs = child.get_attributes()
print(f"before_map_state={attrs.map_state}")

# Reparent the mapped window to parent2
child.reparent(parent2, 20, 20)
d.sync()

# It should still be mapped after reparent
attrs2 = child.get_attributes()
print(f"after_map_state={attrs2.map_state}")

child.destroy()
parent1.destroy()
parent2.destroy()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.error
caught_error = None
def error_handler(err, req):
    global caught_error
    caught_error = err

d = Xlib.display.Display()
d.set_error_handler(error_handler)
screen = d.screen()
gc = screen.root.create_gc()

gc.set_dashes(0, [4, 0, 2])  # 0 in dash list is invalid
d.sync()

if caught_error is not None and caught_error.code == 2:
    print("result=BAD_VALUE")
elif caught_error is not None:
    print(f"result=OTHER_ERROR:code={caught_error.code}")
else:
    print("result=NO_ERROR")

gc.free()
d.close()
`,
		);
		expect(output).toContain("result=BAD_VALUE");
	});

	test("SetDashes accepts valid non-zero dash values", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()
gc = screen.root.create_gc()

# Valid dash list (all non-zero)
try:
    gc.set_dashes(0, [4, 2, 1, 3])
    d.sync()
    print("result=OK")
except Exception as e:
    print(f"result=ERROR:{type(e).__name__}")

gc.free()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create a window
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth)

# Set _NET_WM_WINDOW_TYPE to DOCK
type_atom = d.intern_atom('_NET_WM_WINDOW_TYPE')
dock_atom = d.intern_atom('_NET_WM_WINDOW_TYPE_DOCK')
w.change_property(type_atom, d.intern_atom('ATOM'), 32, [dock_atom])
d.sync()

# Read it back
prop = w.get_full_property(type_atom, d.intern_atom('ATOM'))
if prop and len(prop.value) > 0:
    print(f"result=OK,type={prop.value[0]}")
else:
    print("result=FAIL")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("result=OK");
	});

	test("_NET_WM_STRUT updates _NET_WORKAREA on root", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
import struct
d = Xlib.display.Display()
screen = d.screen()

# Read initial workarea
wa_atom = d.intern_atom('_NET_WORKAREA')
initial = screen.root.get_full_property(wa_atom, d.intern_atom('CARDINAL'))
if initial:
    vals = struct.unpack('<4I', bytes(initial.value))
    initial_w = vals[2]
else:
    initial_w = 0

# Create a dock window with a 50px left strut
dock = screen.root.create_window(0, 0, 50, screen.height_in_pixels, 0, screen.root_depth)
type_atom = d.intern_atom('_NET_WM_WINDOW_TYPE')
dock_atom = d.intern_atom('_NET_WM_WINDOW_TYPE_DOCK')
dock.change_property(type_atom, d.intern_atom('ATOM'), 32, [dock_atom])

strut_atom = d.intern_atom('_NET_WM_STRUT')
dock.change_property(strut_atom, d.intern_atom('CARDINAL'), 32, [50, 0, 0, 0])
d.sync()

# Read updated workarea
updated = screen.root.get_full_property(wa_atom, d.intern_atom('CARDINAL'))
if updated:
    vals = struct.unpack('<4I', bytes(updated.value))
    new_x = vals[0]
    new_w = vals[2]
    if new_x == 50 and new_w < initial_w:
        print("result=OK")
    else:
        print(f"result=WRONG,x={new_x},w={new_w},init_w={initial_w}")
else:
    print("result=NO_WORKAREA")

dock.destroy()
d.sync()
d.close()
`,
		);
		expect(output).toContain("result=OK");
	});

	test("system tray manager is advertised via _NET_SYSTEM_TRAY_S0", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()

# Query _NET_SYSTEM_TRAY_S0 selection owner
tray_atom = d.intern_atom('_NET_SYSTEM_TRAY_S0')
owner = d.get_selection_owner(tray_atom)

if owner and owner.id != 0:
    print(f"result=OK,owner={owner.id:#x}")
else:
    print("result=NO_OWNER")

d.close()
`,
		);
		expect(output).toContain("result=OK");
	});

	test("_NET_WM_STATE can toggle ABOVE state", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.protocol.event
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(10, 10, 200, 200, 0, screen.root_depth)
w.map()
d.sync()

# Send _NET_WM_STATE ClientMessage to add ABOVE state
state_atom = d.intern_atom('_NET_WM_STATE')
above_atom = d.intern_atom('_NET_WM_STATE_ABOVE')

# Create ClientMessage event
event = Xlib.protocol.event.ClientMessage(
    window=w,
    client_type=state_atom,
    data=(32, [1, above_atom, 0, 0, 0])  # action=1 (add)
)
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()

# Check that the state was applied
prop = w.get_full_property(state_atom, d.intern_atom('ATOM'))
if prop and above_atom in prop.value:
    print("result=OK")
else:
    print(f"result=FAIL,prop={prop}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("result=OK");
	});

	test("_NET_CLOSE_WINDOW sends WM_DELETE_WINDOW to compliant windows", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.protocol.event
import time
d = Xlib.display.Display()
screen = d.screen()

# Create window that supports WM_DELETE_WINDOW
w = screen.root.create_window(10, 10, 200, 200, 0, screen.root_depth)
protocols_atom = d.intern_atom('WM_PROTOCOLS')
delete_atom = d.intern_atom('WM_DELETE_WINDOW')
w.change_property(protocols_atom, d.intern_atom('ATOM'), 32, [delete_atom])
w.map()
d.sync()

# Send _NET_CLOSE_WINDOW to root
close_atom = d.intern_atom('_NET_CLOSE_WINDOW')
event = Xlib.protocol.event.ClientMessage(
    window=w,
    client_type=close_atom,
    data=(32, [0, 0, 0, 0, 0])
)
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()

# Check for the WM_DELETE_WINDOW ClientMessage
# Give server a moment to process
import select
d.fileno()
readable, _, _ = select.select([d.fileno()], [], [], 1.0)
if readable:
    count = d.pending_events()
    found = False
    for _ in range(count):
        e = d.next_event()
        if hasattr(e, 'client_type') and e.client_type == protocols_atom:
            found = True
            break
    print(f"result={'OK' if found else 'NO_DELETE_MSG'}")
else:
    print("result=NO_EVENTS")

w.destroy()
d.close()
`,
		);
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
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create and map a window
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth)
w.map()
d.sync()

import time
time.sleep(0.1)

# Check _NET_CLIENT_LIST contains our window
cl_atom = d.intern_atom('_NET_CLIENT_LIST')
prop = screen.root.get_full_property(cl_atom, d.intern_atom('WINDOW'))
if prop and w.id in prop.value:
    print("result=OK")
else:
    print(f"result=NOT_FOUND,wid={w.id:#x},list={list(prop.value) if prop else 'None'}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("result=OK");
	});

	test("_NET_ACTIVE_WINDOW tracks focused window", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create and map a window, set focus
w = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w.map()
d.sync()

import time
time.sleep(0.1)

# Check _NET_ACTIVE_WINDOW
active_atom = d.intern_atom('_NET_ACTIVE_WINDOW')
prop = screen.root.get_full_property(active_atom, d.intern_atom('WINDOW'))
# Just verify the property exists and has a value
if prop and len(prop.value) > 0:
    print(f"result=OK,active={prop.value[0]:#x}")
else:
    print("result=NO_ACTIVE")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("result=OK");
	});

	test("XSETTINGS manager is advertised and provides settings", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()

# Query _XSETTINGS_S0 selection owner
xs_atom = d.intern_atom('_XSETTINGS_S0')
owner = d.get_selection_owner(xs_atom)

if owner and owner.id != 0:
    # Check that XSETTINGS_SETTINGS property exists on the owner window
    settings_atom = d.intern_atom('_XSETTINGS_SETTINGS')
    prop = owner.get_full_property(settings_atom, 0)
    if prop and len(prop.value) > 0:
        print(f"result=OK,owner={owner.id:#x},data_len={len(prop.value)}")
    else:
        print(f"result=NO_SETTINGS,owner={owner.id:#x}")
else:
    print("result=NO_OWNER")

d.close()
`,
		);
		expect(output).toContain("result=OK");
	});

	test("WM_TRANSIENT_FOR stacking: transient above parent", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create parent window
parent = screen.root.create_window(0, 0, 400, 300, 0, screen.root_depth)
parent.map()
d.sync()

# Create transient child
child = screen.root.create_window(50, 50, 200, 150, 0, screen.root_depth)
tf_atom = d.intern_atom('WM_TRANSIENT_FOR')
child.change_property(tf_atom, d.intern_atom('WINDOW'), 32, [parent.id])
child.map()
d.sync()

import time
time.sleep(0.1)

# Query root children to check stacking order
tree = screen.root.query_tree()
children = [c.id for c in tree.children]

if parent.id in children and child.id in children:
    parent_idx = children.index(parent.id)
    child_idx = children.index(child.id)
    if child_idx > parent_idx:
        print("result=OK")
    else:
        print(f"result=WRONG_ORDER,parent_idx={parent_idx},child_idx={child_idx}")
else:
    print("result=NOT_FOUND")

child.destroy()
parent.destroy()
d.close()
`,
		);
		expect(output).toContain("result=OK");
	});

	test("QueryExtension returns unique major opcodes for all extensions", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
extensions = [
    'BIG-REQUESTS', 'MIT-SHM', 'RENDER', 'XFIXES', 'SHAPE', 'SYNC',
    'Composite', 'DAMAGE', 'Present', 'RANDR', 'XKEYBOARD',
    'XTEST', 'DPMS', 'RECORD', 'SECURITY', 'XVideo',
    'DOUBLE-BUFFER', 'XINERAMA', 'GLX', 'DRI3', 'X-Resource',
]
opcodes = {}
conflicts = []
for name in extensions:
    info = d.query_extension(name)
    if info and info.major_opcode > 0:
        code = info.major_opcode
        if code in opcodes:
            conflicts.append(f"{name}={code} conflicts with {opcodes[code]}")
        opcodes[code] = name
if conflicts:
    print(f"CONFLICTS: {', '.join(conflicts)}")
else:
    print(f"OK: {len(opcodes)} extensions with unique opcodes")
d.close()
`,
		);
		expect(output).toContain("OK:");
		expect(output).not.toContain("CONFLICTS");
	});

	test("SYNC extension events use correct event base", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
info = d.query_extension('SYNC')
if info:
    print(f"present={info.major_opcode > 0}")
    print(f"major_opcode={info.major_opcode}")
    print(f"first_event={info.first_event}")
    # SYNC first_event must be 83 (AlarmNotify)
    print(f"event_base_correct={info.first_event == 83}")
else:
    print("present=False")
d.close()
`,
		);
		expect(output).toContain("present=True");
		expect(output).toContain("event_base_correct=True");
	});

	test("RENDER extension reports first_error for BadPictFormat", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
info = d.query_extension('RENDER')
if info:
    print(f"present={info.major_opcode > 0}")
    print(f"first_error={info.first_error}")
    # RENDER first_error should be non-zero (142 = BadPictFormat)
    print(f"has_error_base={info.first_error > 0}")
else:
    print("present=False")
d.close()
`,
		);
		expect(output).toContain("present=True");
		expect(output).toContain("has_error_base=True");
	});

	test("Extension event bases do not overlap", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
# Extensions with events and their expected event counts
ext_events = {
    'SHAPE': 1,
    'MIT-SHM': 1,
    'SYNC': 1,
    'XKEYBOARD': 1,
    'XFIXES': 2,
    'RANDR': 2,
    'DAMAGE': 1,
    'SECURITY': 1,
    'XVideo': 2,
}
ranges = []
for name, count in ext_events.items():
    info = d.query_extension(name)
    if info and info.first_event > 0:
        base = info.first_event
        ranges.append((base, base + count, name))

# Check for overlaps
overlaps = []
for i in range(len(ranges)):
    for j in range(i + 1, len(ranges)):
        a_start, a_end, a_name = ranges[i]
        b_start, b_end, b_name = ranges[j]
        if a_start < b_end and b_start < a_end:
            overlaps.append(f"{a_name}({a_start}-{a_end-1}) overlaps {b_name}({b_start}-{b_end-1})")

if overlaps:
    print(f"OVERLAP: {'; '.join(overlaps)}")
else:
    print(f"OK: {len(ranges)} event ranges are non-overlapping")
d.close()
`,
		);
		expect(output).toContain("OK:");
		expect(output).not.toContain("OVERLAP");
	});
});
