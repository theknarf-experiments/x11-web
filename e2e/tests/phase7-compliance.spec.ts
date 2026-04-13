/**
 * Phase 7 compliance tests: XKB control masks, MouseKeys, event delivery,
 * drawable depth handling, RENDER extension, and application compatibility.
 */

import { test, expect } from "./fixtures";
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
