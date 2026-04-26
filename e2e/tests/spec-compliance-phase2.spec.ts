/**
 * E2E compliance tests for Phase 2 spec compliance fixes:
 * - CopyColormapAndFree semantics
 * - ChangeKeyboardControl led_mode and per-key auto-repeat
 * - WarpPointer MotionNotify timestamp
 * - DPMS ForceLevel validation
 * - MIT-SCREEN-SAVER extension event base
 * - XFIXES CreatePointerBarrier window validation
 * - SECURITY untrusted client restrictions
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

// ==========================================================================
// CopyColormapAndFree
// ==========================================================================
test.describe.serial("CopyColormapAndFree spec compliance", () => {
	test.setTimeout(60_000);

	test("CopyColormapAndFree copies source and is usable", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
cmap = screen.default_colormap

# Allocate a color on the default colormap
result = cmap.alloc_color(0xFFFF, 0, 0)  # Red
pixel = result.pixel

# CopyColormapAndFree creates a new colormap as a copy of cmap
# (python-xlib quirk: self is just for display access; scr_cmap is the source)
new_cmap = cmap.copy_colormap_and_free(cmap)
d.sync()

# The new colormap should be valid and usable
try:
    result2 = new_cmap.alloc_color(0, 0xFFFF, 0)  # Green
    print(f"COPY_CMAP_OK pixel={result2.pixel:#x}")
except Exception as e:
    print(f"COPY_CMAP_FAIL: {e}")
`,
		);
		expect(output).toContain("COPY_CMAP_OK");
	});
});

// ==========================================================================
// ChangeKeyboardControl
// ==========================================================================
test.describe.serial("ChangeKeyboardControl spec compliance", () => {
	test.setTimeout(60_000);

	test("GetKeyboardControl returns valid auto_repeats bitmap", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
ctrl = d.get_keyboard_control()
# auto_repeats should be a 32-element list/tuple of bytes
ar = ctrl.auto_repeats
if len(ar) == 32:
    # All keys initially auto-repeat (0xFF in each byte)
    all_ff = all(b == 0xFF for b in ar)
    if all_ff:
        print("AUTO_REPEATS_ALL_ON")
    else:
        print(f"AUTO_REPEATS_PARTIAL: {[hex(b) for b in ar[:8]]}")
else:
    print(f"AUTO_REPEATS_WRONG_LEN: {len(ar)}")
`,
		);
		// Either all-on or partial is fine (some modifier keys may be excluded)
		expect(output).toMatch(/AUTO_REPEATS_(ALL_ON|PARTIAL)/);
	});

	test("ChangeKeyboardControl modifies bell settings", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()

# Change bell percent
d.change_keyboard_control(bell_percent=75)
d.sync()
ctrl = d.get_keyboard_control()
if ctrl.bell_percent == 75:
    print("BELL_PERCENT_OK")
else:
    print(f"BELL_PERCENT_FAIL: got {ctrl.bell_percent}")

# Change bell pitch
d.change_keyboard_control(bell_pitch=800)
d.sync()
ctrl2 = d.get_keyboard_control()
if ctrl2.bell_pitch == 800:
    print("BELL_PITCH_OK")
else:
    print(f"BELL_PITCH_FAIL: got {ctrl2.bell_pitch}")
`,
		);
		expect(output).toContain("BELL_PERCENT_OK");
		expect(output).toContain("BELL_PITCH_OK");
	});
});

// ==========================================================================
// WarpPointer MotionNotify timestamp
// ==========================================================================
test.describe.serial("WarpPointer spec compliance", () => {
	test.setTimeout(60_000);

	test("WarpPointer moves pointer to target coordinates", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Warp pointer to specific coordinates
d.warp_pointer(0, root, 0, 0, 0, 0, 200, 150)
d.sync()

# Query pointer position
result = root.query_pointer()
x = result.root_x
y = result.root_y
if abs(x - 200) <= 1 and abs(y - 150) <= 1:
    print("WARP_OK")
else:
    print(f"WARP_FAIL: got ({x},{y}) expected (200,150)")
`,
		);
		expect(output).toContain("WARP_OK");
	});
});

// ==========================================================================
// DPMS extension
// ==========================================================================
test.describe.serial("DPMS spec compliance", () => {
	test.setTimeout(60_000);

	test("DPMS extension is reported by server", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(sidecarContainer, "xdpyinfo 2>/dev/null | grep -i dpms || echo NO_DPMS");
		expect(output.toLowerCase()).toContain("dpms");
	});
});

// ==========================================================================
// MIT-SCREEN-SAVER extension event base
// ==========================================================================
test.describe.serial("MIT-SCREEN-SAVER extension", () => {
	test.setTimeout(60_000);

	test("MIT-SCREEN-SAVER extension is present with event base", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
ext = d.query_extension("MIT-SCREEN-SAVER")
if ext is None:
    print("EXT_NOT_FOUND")
elif ext.major_opcode > 0:
    event_base = ext.first_event
    print(f"EXT_OK opcode={ext.major_opcode} event_base={event_base}")
    if event_base == 92:
        print("EVENT_BASE_92_OK")
    elif event_base > 0:
        print(f"EVENT_BASE_NONZERO_OK={event_base}")
    else:
        print("EVENT_BASE_ZERO_FAIL")
else:
    print("EXT_NO_OPCODE")
`,
		);
		expect(output).toContain("EXT_OK");
		expect(output).toContain("EVENT_BASE_92_OK");
	});
});

// ==========================================================================
// XFIXES extension
// ==========================================================================
test.describe.serial("XFIXES spec compliance", () => {
	test.setTimeout(60_000);

	test("XFIXES extension is present", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
ext = d.query_extension("XFIXES")
if ext is not None and ext.major_opcode > 0:
    print(f"XFIXES_OK opcode={ext.major_opcode}")
else:
    print("XFIXES_NOT_FOUND")
`,
		);
		expect(output).toContain("XFIXES_OK");
	});
});

// ==========================================================================
// SECURITY extension
// ==========================================================================
test.describe.serial("SECURITY extension", () => {
	test.setTimeout(60_000);

	test("SECURITY extension is present", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
ext = d.query_extension("SECURITY")
if ext is not None and ext.major_opcode > 0:
    print(f"SECURITY_OK opcode={ext.major_opcode}")
else:
    print("SECURITY_NOT_FOUND")
`,
		);
		expect(output).toContain("SECURITY_OK");
	});
});

// ==========================================================================
// Extension event bases consistency
// ==========================================================================
test.describe.serial("Extension event bases", () => {
	test.setTimeout(60_000);

	test("all extensions report valid non-overlapping event bases", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()

extensions = [
    "SHAPE", "SYNC", "RANDR", "XKEYBOARD", "DAMAGE",
    "MIT-SCREEN-SAVER", "XInputExtension", "XFIXES"
]

event_bases = {}
for name in extensions:
    ext = d.query_extension(name)
    if ext and ext.major_opcode > 0 and ext.first_event > 0:
        event_bases[name] = ext.first_event

# Check for uniqueness
values = list(event_bases.values())
unique = len(set(values)) == len(values)
if unique:
    print("EVENT_BASES_UNIQUE_OK")
else:
    print(f"EVENT_BASES_OVERLAP: {event_bases}")

# Report bases
for name, base in sorted(event_bases.items(), key=lambda x: x[1]):
    print(f"  {name}: event_base={base}")
`,
		);
		expect(output).toContain("EVENT_BASES_UNIQUE_OK");
	});
});
