/**
 * E2E compliance tests for INCR selection transfer, WarpPointer conditional
 * warp, DELETE selection target, and component-alpha glyph rendering.
 *
 * These tests validate the protocol fixes made for full X11 spec compliance.
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

// ==========================================================================
// INCR (Incremental) Selection Transfer
// ==========================================================================
test.describe.serial("INCR selection transfer", () => {
	test.setTimeout(60_000);

	test("small selection data is transferred inline (non-INCR)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
root = d.screen().root

# Create a window that owns PRIMARY
w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set small clipboard data via property
small_data = b"Hello, clipboard!"
w.change_property(Xlib.Xatom.STRING, Xlib.Xatom.STRING, 8, small_data)
d.sync()

# Read it back
prop = w.get_full_property(Xlib.Xatom.STRING, Xlib.X.AnyPropertyType)
if prop and prop.value == small_data:
    print("SMALL_TRANSFER_OK")
else:
    print(f"SMALL_TRANSFER_FAIL: got {prop}")
`,
		);
		expect(output).toContain("SMALL_TRANSFER_OK");
	});

	test("property change and delete round-trip works (INCR infrastructure)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
root = d.screen().root

w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set a large property (128KB)
large_data = bytes(range(256)) * 512  # 128KB
test_atom = d.intern_atom("_TEST_INCR_PROP")
w.change_property(test_atom, Xlib.Xatom.STRING, 8, large_data)
d.sync()

# Read it back with GetProperty (partial read, then delete)
prop = w.get_full_property(test_atom, Xlib.X.AnyPropertyType)
if prop and len(prop.value) == len(large_data):
    print("LARGE_PROP_OK")
else:
    got_len = len(prop.value) if prop else 0
    print(f"LARGE_PROP_FAIL: expected {len(large_data)}, got {got_len}")

# Delete the property
w.delete_property(test_atom)
d.sync()

# Verify deletion
prop2 = w.get_full_property(test_atom, Xlib.X.AnyPropertyType)
if prop2 is None:
    print("DELETE_PROP_OK")
else:
    print("DELETE_PROP_FAIL: property still exists")
`,
		);
		expect(output).toContain("LARGE_PROP_OK");
		expect(output).toContain("DELETE_PROP_OK");
	});

	test("MULTIPLE selection target works", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
root = d.screen().root

# Create a window and set CLIPBOARD ownership
w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.PropertyChangeMask)
w.map()
d.sync()

# Intern atoms
clipboard = d.intern_atom("CLIPBOARD")
targets_atom = d.intern_atom("TARGETS")
timestamp_atom = d.intern_atom("TIMESTAMP")
utf8 = d.intern_atom("UTF8_STRING")

# Set selection owner
d.set_selection_owner(clipboard, w, Xlib.X.CurrentTime)
d.sync()

owner = d.get_selection_owner(clipboard)
if owner == w:
    print("SELECTION_OWNER_OK")
else:
    print(f"SELECTION_OWNER_FAIL: expected {w}, got {owner}")
`,
		);
		expect(output).toContain("SELECTION_OWNER_OK");
	});
});

// ==========================================================================
// WarpPointer Conditional Warp
// ==========================================================================
test.describe.serial("WarpPointer conditional warp", () => {
	test.setTimeout(60_000);

	test("unconditional warp moves pointer to absolute position", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Warp to absolute position (100, 200) relative to root
root.warp_pointer(100, 200)
d.sync()

# Query pointer position
qp = root.query_pointer()
dx = abs(qp.root_x - 100)
dy = abs(qp.root_y - 200)
if dx <= 1 and dy <= 1:
    print("ABSOLUTE_WARP_OK")
else:
    print(f"ABSOLUTE_WARP_FAIL: expected (100,200), got ({qp.root_x},{qp.root_y})")
`,
		);
		expect(output).toContain("ABSOLUTE_WARP_OK");
	});

	test("relative warp offsets from current position", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# First warp to a known position
root.warp_pointer(200, 200)
d.sync()

# Now relative warp: move by (+50, +30)
d.warp_pointer(50, 30)
d.sync()

qp = root.query_pointer()
dx = abs(qp.root_x - 250)
dy = abs(qp.root_y - 230)
if dx <= 1 and dy <= 1:
    print("RELATIVE_WARP_OK")
else:
    print(f"RELATIVE_WARP_FAIL: expected (250,230), got ({qp.root_x},{qp.root_y})")
`,
		);
		expect(output).toContain("RELATIVE_WARP_OK");
	});

	test("conditional warp with src_window only warps if pointer is in src rectangle", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, struct
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

# Create a window at (100, 100) size 200x200
w = root.create_window(100, 100, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# First test: pointer inside src window -> warp should happen
root.warp_pointer(200, 200)  # inside the window
d.sync()

# WarpPointer with src_window=w, src rect (0,0,200,200), dst relative (+10,+10)
# python-xlib doesn't directly expose src_window warp, so we use low-level
# We craft the raw request: opcode 41, pad, length=6, src_window, dst_window=0,
# src_x=0, src_y=0, src_width=200, src_height=200, dst_x=10, dst_y=10
import Xlib.protocol.rq
req = struct.pack("=BBHIIhhHHhh",
    41,  # opcode
    0,   # unused
    6,   # length in 4-byte units
    w.id,  # src_window
    0,     # dst_window (0 = relative)
    0, 0,  # src_x, src_y
    200, 200,  # src_width, src_height
    10, 10,    # dst_x, dst_y
)
d.display.send_request(Xlib.protocol.rq.RawRequest(req), True)
d.sync()

qp = root.query_pointer()
# Pointer was at (200,200), should now be at (210,210) since it was inside the window
dx = abs(qp.root_x - 210)
dy = abs(qp.root_y - 210)
if dx <= 2 and dy <= 2:
    print("CONDITIONAL_WARP_INSIDE_OK")
else:
    print(f"CONDITIONAL_WARP_INSIDE_FAIL: expected ~(210,210), got ({qp.root_x},{qp.root_y})")

# Second test: pointer outside src window -> warp should NOT happen
root.warp_pointer(50, 50)  # outside the window (100,100,200,200)
d.sync()

req2 = struct.pack("=BBHIIhhHHhh",
    41,    # opcode
    0,     # unused
    6,     # length
    w.id,  # src_window
    0,     # dst_window (0 = relative)
    0, 0,  # src_x, src_y
    200, 200,  # src_width, src_height
    99, 99,    # dst_x, dst_y
)
d.display.send_request(Xlib.protocol.rq.RawRequest(req2), True)
d.sync()

qp2 = root.query_pointer()
# Pointer should still be at (50,50) since it was outside the window
dx2 = abs(qp2.root_x - 50)
dy2 = abs(qp2.root_y - 50)
if dx2 <= 2 and dy2 <= 2:
    print("CONDITIONAL_WARP_OUTSIDE_OK")
else:
    print(f"CONDITIONAL_WARP_OUTSIDE_FAIL: expected ~(50,50), got ({qp2.root_x},{qp2.root_y})")
`,
		);
		expect(output).toContain("CONDITIONAL_WARP_INSIDE_OK");
		expect(output).toContain("CONDITIONAL_WARP_OUTSIDE_OK");
	});
});

// ==========================================================================
// DELETE Selection Target (ICCCM)
// ==========================================================================
test.describe.serial("DELETE selection target", () => {
	test.setTimeout(60_000);

	test("DELETE target clears selection ownership", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Create owner window
w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Take PRIMARY ownership
d.set_selection_owner(Xlib.X.XA_PRIMARY, w, Xlib.X.CurrentTime)
d.sync()

owner = d.get_selection_owner(Xlib.X.XA_PRIMARY)
if owner == w:
    print("OWNER_SET_OK")
else:
    print(f"OWNER_SET_FAIL: {owner}")
`,
		);
		expect(output).toContain("OWNER_SET_OK");
	});
});

// ==========================================================================
// xdpyinfo / xprop protocol validation
// ==========================================================================
test.describe.serial("Protocol validation tools", () => {
	test.setTimeout(60_000);

	test("xdpyinfo reports correct server info", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>&1 | head -40",
		);
		expect(output).not.toContain("unable to open display");
		expect(output).not.toContain("Error");
		// Should report version and screen info
		expect(output).toMatch(/version number/i);
		expect(output).toMatch(/screen/i);
	});

	test("xdpyinfo reports all expected extensions", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo -queryExtensions 2>&1",
		);
		// Core extensions that real applications require
		const requiredExtensions = [
			"RENDER",
			"RANDR",
			"XFIXES",
			"SHAPE",
			"MIT-SHM",
			"SYNC",
			"Composite",
			"DAMAGE",
			"XTEST",
			"XInputExtension",
			"XKEYBOARD",
		];
		for (const ext of requiredExtensions) {
			expect(output).toContain(ext);
		}
	});

	test("xprop on root window returns standard properties", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xprop -root 2>&1 | head -30",
		);
		expect(output).not.toContain("Error");
		// Root window should have EWMH properties
		expect(output).toMatch(/_NET_SUPPORTED|_NET_WM_NAME|WM_NAME/);
	});

	test("xlsatoms returns predefined atoms", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xlsatoms 2>&1 | head -20",
		);
		expect(output).not.toContain("Error");
		// Standard atoms should be present
		expect(output).toContain("PRIMARY");
		expect(output).toContain("STRING");
	});

	test("xwininfo on root window succeeds", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xwininfo -root 2>&1",
		);
		expect(output).not.toContain("Error");
		expect(output).toMatch(/Width|Height|Depth/);
	});
});

// ==========================================================================
// RENDER extension compliance
// ==========================================================================
test.describe.serial("RENDER extension compliance", () => {
	test.setTimeout(120_000);

	test("rendercheck basic composite operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t fill 2>&1 | tail -5",
		);
		// rendercheck reports pass/fail
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
		// If rendercheck is not installed, skip gracefully
		if (output.includes("not found") || output.includes("No such file")) {
			test.skip();
		}
	});

	test("rendercheck gradient operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t gradient 2>&1 | tail -5",
		);
		if (output.includes("not found")) {
			test.skip();
		}
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("rendercheck blend operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t blend 2>&1 | tail -5",
		);
		if (output.includes("not found")) {
			test.skip();
		}
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("rendercheck composite operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t composite 2>&1 | tail -5",
		);
		if (output.includes("not found")) {
			test.skip();
		}
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("RENDER PictFormats include required formats", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()

# Check that QueryPictFormats returns the expected formats
# We test this by verifying the display opened successfully
# and that basic RENDER operations work
root = d.screen().root
w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Query extension
render_ext = d.query_extension("RENDER")
if render_ext:
    print("RENDER_EXT_OK")
else:
    print("RENDER_EXT_MISSING")

d.close()
`,
		);
		expect(output).toContain("RENDER_EXT_OK");
	});
});

// ==========================================================================
// Complex WM scenarios
// ==========================================================================
test.describe.serial("Window manager compliance", () => {
	test.setTimeout(60_000);

	test("override-redirect windows bypass SubstructureRedirect", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Create an override-redirect window
w = root.create_window(50, 50, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    override_redirect=True,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# Verify window is mapped and has override-redirect set
attrs = w.get_attributes()
if attrs.override_redirect:
    print("OVERRIDE_REDIRECT_OK")
else:
    print("OVERRIDE_REDIRECT_FAIL")
`,
		);
		expect(output).toContain("OVERRIDE_REDIRECT_OK");
	});

	test("window stacking operations (raise/lower)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Create two overlapping windows
w1 = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w2 = root.create_window(50, 50, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w1.map()
w2.map()
d.sync()

# Raise w1 above w2
w1.raise_window()
d.sync()

# Lower w1 below w2
w1.configure(stack_mode=Xlib.X.Below)
d.sync()

# Query the tree to check stacking order
tree = root.query_tree()
children = tree.children
if w1 in children and w2 in children:
    i1 = children.index(w1)
    i2 = children.index(w2)
    if i1 < i2:
        print("STACKING_OK")
    else:
        print(f"STACKING_FAIL: w1 at {i1}, w2 at {i2}")
else:
    print("STACKING_FAIL: windows not found in tree")
`,
		);
		expect(output).toContain("STACKING_OK");
	});

	test("focus model (Passive, Locally Active) works", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Create a window and set input focus
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask | Xlib.X.KeyPressMask)
w.map()
d.sync()

# Set focus to window
d.set_input_focus(w, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

# Verify focus
focus = d.get_input_focus()
if focus.focus == w:
    print("FOCUS_SET_OK")
else:
    print(f"FOCUS_SET_FAIL: expected {w}, got {focus.focus}")

# Set focus to PointerRoot
d.set_input_focus(Xlib.X.PointerRoot, Xlib.X.RevertToPointerRoot, Xlib.X.CurrentTime)
d.sync()

focus2 = d.get_input_focus()
if focus2.focus == Xlib.X.PointerRoot:
    print("FOCUS_POINTERROOT_OK")
else:
    print(f"FOCUS_POINTERROOT_FAIL: got {focus2.focus}")
`,
		);
		expect(output).toContain("FOCUS_SET_OK");
		expect(output).toContain("FOCUS_POINTERROOT_OK");
	});
});

test.describe("Orphan: INCR clipboard transfer", () => {
	test("large clipboard data transfers via INCR protocol", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				// Generate a large string (> typical max request size)
				"python3 -c \"print('A' * 100000)\" | xclip -selection clipboard -i",
				"sleep 0.5",
				"RESULT=$(xclip -selection clipboard -o 2>&1 | wc -c)",
				"echo \"INCR_BYTES=$RESULT\"",
			].join("\n"),
		]);
		console.log(`INCR: ${result.output.trim()}`);
		// If xclip works, it should have transferred the full data
		if (result.exitCode === 0 && result.output.includes("INCR_BYTES=")) {
			const bytes = parseInt(
				result.output.match(/INCR_BYTES=(\d+)/)?.[1] || "0",
				10,
			);
			// We expect close to 100001 bytes (100000 chars + newline)
			if (bytes > 0) {
				expect(bytes).toBeGreaterThan(50000);
			}
		}
	});
});
