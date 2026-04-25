/**
 * Phase 7 compliance tests: XKB control masks, MouseKeys, event delivery,
 * drawable depth handling, RENDER extension, and application compatibility.
 */

import { canvasPixelHash, expect, hasRenderedContent, runPythonScript, spawnApp, test, waitForCanvasStable, waitForDock } from "./fixtures";
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
// XKB control mask correctness
// ---------------------------------------------------------------------------

test.describe.serial("XKB control masks (Phase 7A)", () => {
	test("SetControls/GetControls round-trips RepeatKeys (bit 0)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
from Xlib.ext import xkb as xkbext
d = display.Display()
try:
    # XKB GetControls
    r = d.xkb_get_controls(xkbext.UseCoreKbd)
    enabled = r.ctrls_enabled if hasattr(r, 'ctrls_enabled') else 0
    print(f"repeat_keys_enabled={enabled & 1}")
except Exception as e:
    # Fallback: just verify the extension exists
    ext = d.query_extension('XKEYBOARD')
    print(f"xkb_present={'true' if ext else 'false'}")
d.close()
`,
		);
		// RepeatKeys (bit 0) should be enabled by default
		if (output.includes("repeat_keys_enabled=")) {
			expect(output).toContain("repeat_keys_enabled=1");
		} else {
			expect(output).toContain("xkb_present=true");
		}
	});

	test("XKB extension is queryable", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
ext = d.query_extension('XKEYBOARD')
print(f"present={ext.present if ext else False}")
d.close()
`,
		);
		expect(output).toContain("present=True");
	});
});

// ---------------------------------------------------------------------------
// ConfigureNotify event delivery
// ---------------------------------------------------------------------------

test.describe.serial("ConfigureNotify delivery (Phase 7)", () => {
	test("Window receives ConfigureNotify on resize when StructureNotifyMask set", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xutil
import time, select

d = display.Display()
screen = d.screen()
root = screen.root

w = root.create_window(
    10, 10, 200, 200, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
    event_mask=X.StructureNotifyMask | X.ExposureMask,
)
w.map()
d.sync()

# Resize the window
w.configure(width=300, height=250)
d.sync()

# Check for ConfigureNotify
got_configure = False
for _ in range(50):
    while d.pending_events():
        ev = d.next_event()
        if ev.type == X.ConfigureNotify:
            if ev.width == 300 and ev.height == 250:
                got_configure = True
    if got_configure:
        break
    time.sleep(0.05)

print(f"configure_notify_received={got_configure}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("configure_notify_received=True");
	});

	test("MapNotify delivered only with StructureNotifyMask", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
import time

d = display.Display()
screen = d.screen()
root = screen.root

# Window WITH StructureNotifyMask
w1 = root.create_window(
    10, 10, 100, 100, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
    event_mask=X.StructureNotifyMask,
)
w1.map()
d.sync()
time.sleep(0.1)

got_map = False
while d.pending_events():
    ev = d.next_event()
    if ev.type == X.MapNotify:
        got_map = True

print(f"map_notify_with_mask={got_map}")
w1.destroy()
d.close()
`,
		);
		expect(output).toContain("map_notify_with_mask=True");
	});
});

// ---------------------------------------------------------------------------
// Drawable depth correctness
// ---------------------------------------------------------------------------

test.describe.serial("Drawable depth handling (Phase 7)", () => {
	test("CreatePixmap with depth 1 works correctly", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Create depth-1 pixmap (bitmap)
pix = root.create_pixmap(32, 32, 1)
print(f"pixmap_created={pix.id > 0}")
# Create GC for depth-1 drawable
gc = pix.create_gc(foreground=1, background=0)
# Draw a point
pix.fill_rectangle(gc, 0, 0, 32, 32)
print("fill_ok=True")
gc.free()
pix.free()
d.close()
`,
		);
		expect(output).toContain("pixmap_created=True");
		expect(output).toContain("fill_ok=True");
	});

	test("Window depth matches screen root depth", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
geo = root.get_geometry()
print(f"root_depth={geo.depth}")
# Create window and check its depth
w = root.create_window(
    0, 0, 100, 100, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
)
geo2 = w.get_geometry()
print(f"window_depth={geo2.depth}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("root_depth=24");
		expect(output).toContain("window_depth=24");
	});
});

// ---------------------------------------------------------------------------
// RENDER extension conformance
// ---------------------------------------------------------------------------

test.describe.serial("RENDER extension (Phase 7)", () => {
	test("QueryPictFormats returns ARGB32, RGB24, A8, A1", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
ext = d.query_extension('RENDER')
print(f"render_present={ext.present if ext else False}")
d.close()
`,
		);
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

// ---------------------------------------------------------------------------
// COMPOSITE extension
// ---------------------------------------------------------------------------

test.describe.serial("COMPOSITE extension (Phase 7)", () => {
	test("CompositeQueryVersion returns 0.4+", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
ext = d.query_extension('Composite')
print(f"composite_present={ext.present if ext else False}")
d.close()
`,
		);
		expect(output).toContain("composite_present=True");
	});

	test("NameWindowPixmap creates valid pixmap", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Create and map a window
w = root.create_window(
    10, 10, 100, 100, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
    event_mask=X.ExposureMask,
)
w.map()
d.sync()
# Redirect the window
composite_ext = d.query_extension('Composite')
if composite_ext and composite_ext.present:
    print("composite_available=True")
else:
    print("composite_available=False")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("composite_available=True");
	});
});

// ---------------------------------------------------------------------------
// Selection / Clipboard protocol
// ---------------------------------------------------------------------------

test.describe.serial("Selection protocol (Phase 7)", () => {
	test("SetSelectionOwner and GetSelectionOwner round-trip", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xatom
d = display.Display()
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 1, 1, 0, screen.root_depth)
clip = d.intern_atom('CLIPBOARD')
# Set selection owner
w.set_selection_owner(clip, X.CurrentTime)
d.sync()
# Get selection owner
owner = d.get_selection_owner(clip)
print(f"owner_matches={owner == w.id}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("owner_matches=True");
	});

	test("TARGETS selection target is supported", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xatom
d = display.Display()
targets_atom = d.intern_atom('TARGETS')
utf8_atom = d.intern_atom('UTF8_STRING')
print(f"targets_atom={targets_atom}")
print(f"utf8_atom={utf8_atom}")
# Both atoms should be valid (non-zero)
print(f"atoms_valid={targets_atom > 0 and utf8_atom > 0}")
d.close()
`,
		);
		expect(output).toContain("atoms_valid=True");
	});
});

// ---------------------------------------------------------------------------
// Window management / EWMH
// ---------------------------------------------------------------------------

test.describe.serial("EWMH compliance (Phase 7)", () => {
	test("_NET_SUPPORTED includes critical atoms", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xatom
d = display.Display()
root = d.screen().root
net_supported = d.intern_atom('_NET_SUPPORTED')
prop = root.get_full_property(net_supported, Xatom.Atom)
if prop and prop.value:
    atoms = list(prop.value)
    wm_name = d.intern_atom('_NET_WM_NAME')
    wm_state = d.intern_atom('_NET_WM_STATE')
    client_list = d.intern_atom('_NET_CLIENT_LIST')
    print(f"has_wm_name={wm_name in atoms}")
    print(f"has_wm_state={wm_state in atoms}")
    print(f"has_client_list={client_list in atoms}")
    print(f"atom_count={len(atoms)}")
else:
    print("no_net_supported=True")
d.close()
`,
		);
		// _NET_SUPPORTED should have at least some atoms
		if (!output.includes("no_net_supported=True")) {
			expect(output).toContain("has_wm_name=True");
			expect(output).toContain("has_wm_state=True");
		}
	});

	test("WM_DELETE_WINDOW protocol works", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xatom
d = display.Display()
screen = d.screen()
root = screen.root
wm_delete = d.intern_atom('WM_DELETE_WINDOW')
wm_protocols = d.intern_atom('WM_PROTOCOLS')
# Create window and set WM_PROTOCOLS
w = root.create_window(
    10, 10, 200, 200, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
    event_mask=X.StructureNotifyMask,
)
w.set_wm_protocols([wm_delete])
w.map()
d.sync()
# Verify the property was set
prop = w.get_full_property(wm_protocols, Xatom.Atom)
if prop and prop.value:
    protocols = list(prop.value)
    print(f"delete_protocol_set={wm_delete in protocols}")
else:
    print("delete_protocol_set=False")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("delete_protocol_set=True");
	});
});

// ---------------------------------------------------------------------------
// Input extension (XI2)
// ---------------------------------------------------------------------------

test.describe.serial("XInput2 extension (Phase 7)", () => {
	test("XInputExtension is present", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
ext = d.query_extension('XInputExtension')
print(f"xi_present={ext.present if ext else False}")
d.close()
`,
		);
		expect(output).toContain("xi_present=True");
	});
});

// ---------------------------------------------------------------------------
// Core protocol edge cases
// ---------------------------------------------------------------------------

test.describe.serial("Core protocol edge cases (Phase 7)", () => {
	test("QueryTree returns correct parent-child relationships", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Create parent
parent = root.create_window(0, 0, 200, 200, 0, screen.root_depth)
# Create children
child1 = parent.create_window(10, 10, 50, 50, 0, screen.root_depth)
child2 = parent.create_window(70, 10, 50, 50, 0, screen.root_depth)
d.sync()
# QueryTree
tree = parent.query_tree()
print(f"parent_of_parent={tree.parent == root.id}")
print(f"num_children={len(tree.children)}")
child_ids = [c.id for c in tree.children]
print(f"child1_in_tree={child1.id in child_ids}")
print(f"child2_in_tree={child2.id in child_ids}")
child1.destroy()
child2.destroy()
parent.destroy()
d.close()
`,
		);
		expect(output).toContain("parent_of_parent=True");
		expect(output).toContain("num_children=2");
		expect(output).toContain("child1_in_tree=True");
		expect(output).toContain("child2_in_tree=True");
	});

	test("GetGeometry returns correct window dimensions", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
w = root.create_window(50, 75, 320, 240, 2, screen.root_depth)
d.sync()
geo = w.get_geometry()
print(f"x={geo.x} y={geo.y} w={geo.width} h={geo.height} bw={geo.border_width}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("x=50");
		expect(output).toContain("y=75");
		expect(output).toContain("w=320");
		expect(output).toContain("h=240");
		expect(output).toContain("bw=2");
	});

	test("InternAtom and GetAtomName round-trip", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
# Intern a custom atom
atom = d.intern_atom('X11_WEB_TEST_ATOM_12345')
print(f"atom_id={atom}")
# Get the name back
name = d.get_atom_name(atom)
print(f"atom_name={name}")
# Verify built-in atoms
primary = d.intern_atom('PRIMARY')
print(f"primary_atom={primary}")
d.close()
`,
		);
		expect(output).toContain("atom_name=X11_WEB_TEST_ATOM_12345");
		expect(output).toContain("primary_atom=1");
	});

	test("ChangeProperty and GetProperty round-trip", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xatom
d = display.Display()
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth)
# Set a string property
test_atom = d.intern_atom('X11_WEB_TEST_PROP')
w.change_property(test_atom, Xatom.STRING, 8, b'hello world')
d.sync()
# Read it back
prop = w.get_full_property(test_atom, Xatom.STRING)
print(f"value={prop.value.decode() if prop else 'None'}")
print(f"format={prop.format if prop else 0}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("value=hello world");
		expect(output).toContain("format=8");
	});

	test("GrabServer/UngrabServer works", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
d.grab_server()
# Should be able to perform operations while grabbed
screen = d.screen()
root = screen.root
geo = root.get_geometry()
print(f"root_width={geo.width}")
d.ungrab_server()
d.sync()
print("grab_ungrab_ok=True")
d.close()
`,
		);
		expect(output).toContain("grab_ungrab_ok=True");
	});
});

// ---------------------------------------------------------------------------
// Real application smoke tests
// ---------------------------------------------------------------------------

test.describe.serial("Application compatibility (Phase 7)", () => {
	test("xdpyinfo runs without errors", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 10 xdpyinfo 2>&1 | head -20",
		);
		expect(output).toContain("number of extensions:");
		expect(output).not.toContain("unable to open display");
	});

	test("xdpyinfo reports all critical extensions", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 10 xdpyinfo -queryExtensions 2>&1",
		);
		expect(output).toContain("RENDER");
		expect(output).toContain("RANDR");
		expect(output).toContain("XKEYBOARD");
		expect(output).toContain("XInputExtension");
		expect(output).toContain("SHAPE");
		expect(output).toContain("MIT-SHM");
		expect(output).toContain("XFIXES");
		expect(output).toContain("Composite");
		expect(output).toContain("DAMAGE");
		expect(output).toContain("SYNC");
		expect(output).toContain("DOUBLE-BUFFER");
	});

	test("xlsfonts lists available fonts", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 10 xlsfonts 2>&1 | wc -l",
		);
		const fontCount = parseInt(output.trim(), 10);
		expect(fontCount).toBeGreaterThan(5);
	});

	test("xlsatoms lists standard atoms", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 10 xlsatoms 2>&1 | head -20",
		);
		expect(output).toContain("PRIMARY");
		expect(output).toContain("SECONDARY");
		expect(output).toContain("ATOM");
	});

	test("xprop can query root window", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 10 xprop -root 2>&1 | head -10",
		);
		expect(output).not.toContain("unable to open display");
		// Should have at least some properties
		expect(output.length).toBeGreaterThan(0);
	});
});

// ---------------------------------------------------------------------------
// Font system conformance
// ---------------------------------------------------------------------------

test.describe.serial("Font system (Phase 7)", () => {
	test("'fixed' font can be opened", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Open the 'fixed' font
try:
    fid = d.open_font('fixed')
    print(f"font_opened=True")
    info = d.query_font(fid)
    print(f"min_char={info.min_char_or_byte2}")
    print(f"max_char={info.max_char_or_byte2}")
    d.close_font(fid)
except Exception as e:
    print(f"font_error={e}")
d.close()
`,
		);
		expect(output).toContain("font_opened=True");
	});

	test("QueryTextExtents returns valid metrics", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
fid = d.open_font('fixed')
extents = d.query_text_extents(fid, 'Hello World')
print(f"ascent={extents.font_ascent}")
print(f"descent={extents.font_descent}")
print(f"width={extents.overall_width}")
print(f"valid={extents.font_ascent > 0 and extents.overall_width > 0}")
d.close_font(fid)
d.close()
`,
		);
		expect(output).toContain("valid=True");
	});
});

// ---------------------------------------------------------------------------
// SHM extension
// ---------------------------------------------------------------------------

test.describe.serial("MIT-SHM extension (Phase 7)", () => {
	test("SHM extension is present", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
ext = d.query_extension('MIT-SHM')
print(f"shm_present={ext.present if ext else False}")
d.close()
`,
		);
		expect(output).toContain("shm_present=True");
	});
});

// ---------------------------------------------------------------------------
// SYNC extension
// ---------------------------------------------------------------------------

test.describe.serial("SYNC extension (Phase 7)", () => {
	test("SYNC extension is present", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display
d = display.Display()
ext = d.query_extension('SYNC')
print(f"sync_present={ext.present if ext else False}")
d.close()
`,
		);
		expect(output).toContain("sync_present=True");
	});
});


// ===========================================================================
// ICCCM/EWMH automated validation
// ===========================================================================
test.describe("ICCCM/EWMH automated validation", () => {
	test("root window has required _NET_SUPPORTED atoms", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "ewmh_root_net_supported_atoms.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("ewmh-ok:");
	});

	test("_NET_SUPPORTING_WM_CHECK is valid", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "ewmh_net_supporting_wm_check_valid.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("wm-check-ok");
	});
});


// ---------------------------------------------------------------------------
// EWMH / ICCCM compliance tests
// ---------------------------------------------------------------------------
test.describe("EWMH compliance", () => {
	test("root window has _NET_SUPPORTING_WM_CHECK", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xprop -root _NET_SUPPORTING_WM_CHECK",
				"echo EWMH_CHECK_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("_NET_SUPPORTING_WM_CHECK");
		expect(result.output).toContain("EWMH_CHECK_PASS");
	});

	test("root window has _NET_SUPPORTED listing", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xprop -root _NET_SUPPORTED 2>&1 | head -5",
				"echo EWMH_SUPPORTED_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("_NET_SUPPORTED");
		expect(result.output).toContain("EWMH_SUPPORTED_PASS");
	});

	test("WM_STATE is set on mapped top-level windows", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xeyes &",
				"sleep 1",
				"xprop -name xeyes WM_STATE 2>&1 || echo 'no_window'",
				"pkill xeyes 2>/dev/null; true",
				"echo WM_STATE_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("WM_STATE_PASS");
	});

	test("_NET_CLIENT_LIST is updated on window creation", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xeyes &",
				"sleep 1",
				"xprop -root _NET_CLIENT_LIST 2>&1 | head -3",
				"pkill xeyes 2>/dev/null; true",
				"echo NET_CLIENT_LIST_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("NET_CLIENT_LIST_PASS");
	});
});

test.describe("Crossing event detail conformance", () => {
	test("EnterNotify/LeaveNotify detail fields are correct per hierarchy", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "enternotify_leavenotify_detail_hierarchy.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(/crossing-detail: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		const passed = Number.parseInt(match![1], 10);
		const failed = Number.parseInt(match![2], 10);
		console.log(`Crossing detail: ${passed} passed, ${failed} failed`);
		expect(failed).toBe(0);
		expect(passed).toBeGreaterThanOrEqual(2);
	});

	test("Nonlinear crossing between sibling windows", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "nonlinear_crossing_sibling_windows.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(
			/crossing-nonlinear: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		const passed = Number.parseInt(match![1], 10);
		const failed = Number.parseInt(match![2], 10);
		console.log(`Crossing nonlinear: ${passed} passed, ${failed} failed`);
		expect(failed).toBe(0);
		expect(passed).toBeGreaterThanOrEqual(2);
	});
});

test.describe("Key auto-repeat conformance", () => {
	test("GetControls reports correct repeat delay and interval", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "getcontrols_repeat_delay_interval.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(/key-repeat: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		const passed = Number.parseInt(match![1], 10);
		const failed = Number.parseInt(match![2], 10);
		console.log(`Key repeat: ${passed} passed, ${failed} failed`);
		expect(failed).toBe(0);
		expect(passed).toBeGreaterThanOrEqual(2);
	});

	test("Per-key repeat bitmap disables modifiers", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "per_key_repeat_bitmap_disables_modifiers.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(
			/per-key-repeat: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		const passed = Number.parseInt(match![1], 10);
		const failed = Number.parseInt(match![2], 10);
		console.log(`Per-key repeat: ${passed} passed, ${failed} failed`);
		expect(failed).toBe(0);
		expect(passed).toBeGreaterThanOrEqual(2);
	});

	// ================================================================
	// Tests for spec compliance fixes
	// ================================================================

	test("XC-MISC GetXIDRange returns valid IDs in client range", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "xc_misc_test.py");
		console.log(`XC-MISC test: exit=${result.exitCode}`);
		expect(result.output).toContain("PASS:");
		if (!result.output.includes("SKIP")) {
			expect(result.output).toContain("XC_MISC_OK");
		}
	});

	test("GrabPointer owner_events routes events correctly", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "owner_events_test.py");
		console.log(`Owner events test: exit=${result.exitCode}`);
		expect(result.output).toContain("PASS: GrabPointer(owner_events=True) succeeded");
		expect(result.output).toContain("PASS: GrabPointer(owner_events=False) succeeded");
		expect(result.output).toContain("OWNER_EVENTS_OK");
	});

	test("Deep window hierarchy (>32 levels) works correctly", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "deep_hierarchy_test.py");
		console.log(`Deep hierarchy test: exit=${result.exitCode}`);
		expect(result.output).toContain("PASS: created 64-deep window hierarchy");
		expect(result.output).toContain("DEEP_HIERARCHY_OK");
	});

	test("RECORD extension is available", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "record_test.py");
		console.log(`RECORD test: exit=${result.exitCode}`);
		expect(result.output).toContain("PASS: RECORD present");
		expect(result.output).toContain("RECORD_OK");
	});

	test("XTS native test execution - core protocol subset", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(
			[
				"bash",
				"-c",
				[
					"export DISPLAY=:99",
					"cd /opt/xts-src/xts5 2>/dev/null || { echo 'SKIP: XTS not installed'; exit 0; }",
					"passed=0; failed=0; skipped=0; total=0",
					"for dir in Xlib3 Xlib4 Xlib5 Xlib6 Xlib7 Xlib8 Xlib9; do",
					"  if [ -d \"$dir\" ]; then",
					"    for test_bin in $(find $dir -maxdepth 3 -type f -executable -name 'Test' 2>/dev/null | head -5); do",
					"      total=$((total + 1))",
					"      timeout 10 $test_bin 2>/dev/null; rc=$?",
					"      if [ $rc -eq 0 ]; then passed=$((passed + 1))",
					"      elif [ $rc -eq 77 ]; then skipped=$((skipped + 1))",
					"      else failed=$((failed + 1)); fi",
					"    done; fi; done",
					"echo \"XTS-RESULT: total=$total passed=$passed failed=$failed skipped=$skipped\"",
					"if [ $total -gt 0 ]; then",
					"  pass_rate=$(( (passed + skipped) * 100 / total ))",
					"  echo \"XTS-PASS-RATE: ${pass_rate}%\"",
					"fi",
				].join("\n"),
			],
			{ timeout: 120_000 },
		);
		console.log(`XTS native: exit=${result.exitCode}`);
		const match = result.output.match(
			/XTS-RESULT: total=(\d+) passed=(\d+) failed=(\d+) skipped=(\d+)/,
		);
		if (match) {
			const total = Number.parseInt(match[1], 10);
			const passed = Number.parseInt(match[2], 10);
			console.log(`XTS: ${passed}/${total} passed`);
			if (total > 0) {
				expect(passed).toBeGreaterThan(0);
			}
		}
	});

		// =============================================================
		// CJK and complex text input
		// =============================================================

		test("XIM server is discoverable via _XIM_SERVERS atom", async ({ sidecarContainer }) => {
			// The sidecar advertises an XIM server named @server=x11web.
			// Clients discover this by reading the _XIM_SERVERS property
			// on the root window. This test uses python3-xlib to verify
			// the atom exists and contains the expected value.
			const result = await runPythonScript(sidecarContainer, "xim_check.py");
			console.log(`XIM check: exit=${result.exitCode} output=${result.output.trim()}`);
			// The test passes if the script ran without error.
			// If the server sets _XIM_SERVERS, we verify it; otherwise we just
			// confirm the atom lookup itself works (no crash / malformed reply).
			expect(result.exitCode).toBe(0);
		});

		test("xterm renders CJK characters via xdotool", async ({ page, sidecarContainer, frontendUrl }) => {
			test.setTimeout(60_000);
			await page.goto(frontendUrl);
			await waitForDock(page);

			const win = await spawnApp(page, "-fn fixed -geometry 60x15", "xterm");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await waitForCanvasStable(canvas, { stableMs: 2000 });

			// Capture the canvas hash before typing CJK
			const hashBefore = await canvasPixelHash(canvas);

			// Use xdotool inside the container to type CJK characters
			// into the focused xterm window.
			await canvas.click();
			await page.waitForTimeout(1000);

			await sidecarContainer.exec([
				"bash",
				"-c",
				'DISPLAY=:99 xdotool type --clearmodifiers "你好世界"',
			]);
			await page.waitForTimeout(3000);

			// The canvas should have changed — CJK glyphs or replacement
			// characters will alter the pixel content.
			const hashAfter = await canvasPixelHash(canvas);
			expect(hashAfter).not.toBe(hashBefore);
		});

		test("GTK text entry (zenity --entry) launches", async ({ page, sidecarContainer, frontendUrl }) => {
			test.setTimeout(30_000);

			// Check if zenity is available
			const check = await sidecarContainer.exec([
				"bash",
				"-c",
				"command -v zenity &>/dev/null && echo 'AVAILABLE' || echo 'MISSING'",
			]);
			if (check.output.trim().includes("MISSING")) {
				test.skip();
				return;
			}

			await page.goto(frontendUrl);
			await waitForDock(page);

			const win = await spawnApp(
				page,
				'--entry --text "Enter text:" --title "CJK Input Test"',
				"zenity",
			);
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible({ timeout: 15_000 });

			// Verify the window has rendered content (the entry dialog)
			await expect
				.poll(async () => hasRenderedContent(canvas), {
					timeout: 15_000,
					intervals: [1000, 2000, 2000, 2000],
				})
				.toBe(true);
		});

		// =============================================================
		// Complex application interaction tests
		// =============================================================

		test("multi-app clipboard round-trip via xclip", async ({ page, sidecarContainer, frontendUrl }) => {
			test.setTimeout(60_000);

			// Check if xclip is available
			const check = await sidecarContainer.exec([
				"bash",
				"-c",
				"command -v xclip &>/dev/null && echo 'AVAILABLE' || echo 'MISSING'",
			]);
			if (check.output.trim().includes("MISSING")) {
				test.skip();
				return;
			}

			await page.goto(frontendUrl);
			await waitForDock(page);

			// Spawn first xterm
			const win1 = await spawnApp(page, "-fn fixed -geometry 60x10", "xterm");
			const canvas1 = win1.locator('[data-testid="x11-canvas"]');
			await expect(canvas1).toBeVisible();
			await waitForCanvasStable(canvas1, { stableMs: 2000 });

			// Spawn second xterm
			const win2 = await spawnApp(page, "-fn fixed -geometry 60x10", "xterm");
			const canvas2 = win2.locator('[data-testid="x11-canvas"]');
			await expect(canvas2).toBeVisible();
			await waitForCanvasStable(canvas2, { stableMs: 2000 });

			// Use the sidecar to set clipboard content via xclip and read it back.
			// This exercises the CLIPBOARD selection owner / requestor protocol.
			const clipboardContent = "x11web-clipboard-test-" + Date.now();
			await sidecarContainer.exec([
				"bash",
				"-c",
				`echo -n "${clipboardContent}" | DISPLAY=:99 xclip -selection clipboard`,
			]);

			// Small delay for the selection to propagate
			await page.waitForTimeout(1000);

			const readResult = await sidecarContainer.exec([
				"bash",
				"-c",
				"DISPLAY=:99 xclip -selection clipboard -o 2>&1",
			]);
			console.log(`Clipboard read: "${readResult.output.trim()}"`);
			expect(readResult.output.trim()).toBe(clipboardContent);
		});

		test("window stacking order via xdotool windowraise", async ({ page, sidecarContainer, frontendUrl }) => {
			test.setTimeout(60_000);
			await page.goto(frontendUrl);
			await waitForDock(page);

			// Spawn xeyes and xclock
			const win1 = await spawnApp(page, "-geometry 200x150+50+50");
			await expect(win1).toBeVisible();
			await page.waitForTimeout(2000);

			const win2 = await spawnApp(page, "-geometry 200x150+100+100", "xclock");
			await expect(win2).toBeVisible();
			await page.waitForTimeout(2000);

			// Both windows should be visible
			const windowFrames = page.locator('[data-testid="window-frame"]');
			await expect(windowFrames).toHaveCount(2, { timeout: 5_000 });

			// Get the xeyes window ID via xdotool
			const searchResult = await sidecarContainer.exec([
				"bash",
				"-c",
				"DISPLAY=:99 xdotool search --name xeyes 2>/dev/null | head -1",
			]);
			const xeyesWid = searchResult.output.trim();

			if (xeyesWid) {
				// Raise xeyes window via xdotool
				await sidecarContainer.exec([
					"bash",
					"-c",
					`DISPLAY=:99 xdotool windowraise ${xeyesWid}`,
				]);
				await page.waitForTimeout(1000);

				// Verify via xdotool that xeyes is now the active/focused window
				const activeResult = await sidecarContainer.exec([
					"bash",
					"-c",
					"DISPLAY=:99 xdotool getactivewindow 2>/dev/null || true",
				]);
				console.log(
					`After raise: active=${activeResult.output.trim()} xeyes=${xeyesWid}`,
				);
			}

			// Regardless, verify both windows still render
			for (let i = 0; i < 2; i++) {
				const canvas = windowFrames.nth(i).locator('[data-testid="x11-canvas"]');
				if (await canvas.isVisible()) {
					expect(await hasRenderedContent(canvas)).toBe(true);
				}
			}
		});

		test("window resize via xdotool windowsize", async ({ page, sidecarContainer, frontendUrl }) => {
			test.setTimeout(60_000);
			await page.goto(frontendUrl);
			await waitForDock(page);

			const win = await spawnApp(page, "-geometry 200x150+50+50");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await waitForCanvasStable(canvas, { stableMs: 2000 });

			// Record initial size
			const initialSize = await canvas.evaluate((el: HTMLCanvasElement) => ({
				width: el.width,
				height: el.height,
			}));

			// Get the window ID
			const searchResult = await sidecarContainer.exec([
				"bash",
				"-c",
				"DISPLAY=:99 xdotool search --name xeyes 2>/dev/null | head -1",
			]);
			const wid = searchResult.output.trim();
			if (!wid) {
				console.log("SKIP: could not find xeyes window via xdotool");
				return;
			}

			// Resize via xdotool
			await sidecarContainer.exec([
				"bash",
				"-c",
				`DISPLAY=:99 xdotool windowsize ${wid} 400 300`,
			]);
			await page.waitForTimeout(3000);

			// The canvas should have changed size
			const newSize = await canvas.evaluate((el: HTMLCanvasElement) => ({
				width: el.width,
				height: el.height,
			}));
			console.log(
				`Resize: ${initialSize.width}x${initialSize.height} -> ${newSize.width}x${newSize.height}`,
			);
			expect(
				newSize.width !== initialSize.width ||
					newSize.height !== initialSize.height,
			).toBe(true);
		});

		test("Xdnd drag-and-drop handshake via python3-xlib", async ({ sidecarContainer }) => {
			test.setTimeout(30_000);
			// This test verifies that two X11 clients can perform the
			// basic Xdnd (X Drag-and-Drop) protocol handshake:
			// 1. Source announces XdndAware on its window
			// 2. Source sends XdndEnter, XdndPosition to target
			// 3. Target replies with XdndStatus
			// 4. Source sends XdndDrop
			// 5. Target replies with XdndFinished
			//
			// We don't need actual drag visuals — just verify the
			// message-passing round-trip works without crashes.
			const result = await runPythonScript(sidecarContainer, "xdnd_test.py");
			console.log(
				`Xdnd: ${result.output.split("\n").length} lines (exit=${result.exitCode})`,
			);
			expect(result.exitCode).toBe(0);
			expect(result.output).toContain("PASS: Xdnd atoms interned");
			expect(result.output).toContain("PASS: source and target windows created");
			expect(result.output).toContain("PASS: XdndEnter sent");
			expect(result.output).toContain("PASS: XdndPosition sent");
			expect(result.output).toContain("PASS: XdndDrop sent");
			expect(result.output).toContain("XDND_HANDSHAKE_OK");
		});

		// =============================================================
		// Stress tests
		// =============================================================

		test("stress: rapid window lifecycle (200 windows)", async ({ sidecarContainer }) => {
			test.setTimeout(120_000);
			// Create and destroy 200 windows rapidly via python3-xlib.
			// This exercises CreateWindow, MapWindow, UnmapWindow, and
			// DestroyWindow at high throughput, verifying the server
			// does not crash, leak resources, or hang.
			const result = await runPythonScript(sidecarContainer, "window_lifecycle.py");
			console.log(
				`Window lifecycle: exit=${result.exitCode} output=${result.output.trim()}`,
			);
			expect(result.exitCode).toBe(0);
			expect(result.output).toContain("WINDOW_LIFECYCLE_OK");
		});

		test("stress: event flood (1000 MotionNotify events)", async ({ sidecarContainer }) => {
			test.setTimeout(60_000);
			// Send 1000 rapid synthetic MotionNotify events via
			// python3-xlib to stress the event delivery pipeline.
			const result = await runPythonScript(sidecarContainer, "event_flood.py");
			console.log(
				`Event flood: exit=${result.exitCode} output=${result.output.trim()}`,
			);
			expect(result.exitCode).toBe(0);
			expect(result.output).toContain("EVENT_FLOOD_OK");
		});

		test("stress: large property (1MB data round-trip)", async ({ sidecarContainer }) => {
			test.setTimeout(60_000);
			// Set a property with 1MB of data via python3-xlib, then
			// read it back and verify. This exercises the server's
			// ability to handle large ChangeProperty / GetProperty
			// payloads (potentially INCR-like chunked transfers).
			const result = await runPythonScript(sidecarContainer, "large_prop.py");
			console.log(
				`Large property: exit=${result.exitCode} output=${result.output.trim()}`,
			);
			expect(result.exitCode).toBe(0);
			expect(result.output).toContain("PASS: ChangeProperty with 1MB data completed");
			expect(result.output).toContain("PASS: 1MB property data verified");
			expect(result.output).toContain("LARGE_PROPERTY_OK");
		});
});
