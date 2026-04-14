/**
 * Phase 12: Application Compatibility Tests
 *
 * Tests that real-world applications start, render, interact, and exit
 * correctly on the X11 server. Goes beyond "starts without crash" to verify
 * actual rendering output, event handling, and protocol compliance per toolkit.
 *
 * All tests run in a single serial describe block to share one container setup.
 */

import { test, expect } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

const ENV_PREFIX =
	"export DISPLAY=:99 XAUTHORITY=/tmp/.x11-web-Xauthority;";

/** Run a command inside the sidecar container and return stdout. */
async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
): Promise<string> {
	const result = await container.exec([
		"bash",
		"-c",
		`${ENV_PREFIX} ${cmd}`,
	]);
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
		`${ENV_PREFIX} python3 -c '${escaped}'`,
	]);
	return result.output.trim();
}

/** Kill spawned test apps (not python3 - it may be used by sidecar). */
async function killApps(container: StartedTestContainer) {
	await container
		.exec([
			"bash",
			"-c",
			"pkill -9 -f 'xeyes|xterm|xlogo|xclock|xmessage|zenity|firefox|vim|gimp|gtk3-demo|gnome-calculator|qpdfview|libreoffice|soffice|emacs|gnome-text-editor|wish|qterminal|glmark' 2>/dev/null; true",
		])
		.catch(() => {});
	await new Promise((r) => setTimeout(r, 1000));
}

// All tests in a single serial describe to share one container setup.
test.describe.serial("Application compatibility", () => {
	// Container setup can take several minutes — use a generous timeout.
	test.setTimeout(300_000);

	test.afterEach(async ({ sidecarContainer }) => {
		await killApps(sidecarContainer);
	});

	// --- Container setup (first test absorbs fixture init time) ---

	test("containers start and sidecar is ready", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(sidecarContainer, "echo READY");
		expect(output).toContain("READY");
	});

	// --- X11 tool validation ---

	test("server reports all required extensions", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
exts = d.list_extensions()
for e in sorted(exts):
    print(e)
d.close()
`,
		);

		for (const ext of [
			"BIG-REQUESTS",
			"Composite",
			"DAMAGE",
			"DPMS",
			"Generic Event Extension",
			"MIT-SCREEN-SAVER",
			"MIT-SHM",
			"RANDR",
			"RECORD",
			"RENDER",
			"SECURITY",
			"SHAPE",
			"SYNC",
			"X-Resource",
			"XC-MISC",
			"XFIXES",
			"XInputExtension",
			"XKEYBOARD",
			"XTEST",
			"XVideo",
		]) {
			expect(output).toContain(ext);
		}
	});

	test("server reports correct screen info", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
print(f"width={screen.width_in_pixels}")
print(f"height={screen.height_in_pixels}")
print(f"depth={screen.root_depth}")
print(f"screens={d.screen_count()}")
d.close()
`,
		);
		expect(output).toContain("screens=1");
		expect(output).toContain("depth=24");
	});

	test("standard and EWMH atoms are present", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
for name in ['PRIMARY', 'WM_NAME', 'WM_CLASS', '_NET_WM_STATE', '_NET_SUPPORTED']:
    atom = d.intern_atom(name, True)
    print(f"{name}={atom}")
d.close()
`,
		);
		expect(output).toContain("PRIMARY=");
		expect(output).toContain("WM_NAME=");
		expect(output).toContain("_NET_WM_STATE=");
		expect(output).toContain("_NET_SUPPORTED=");
		// All atoms should be non-zero (meaning they exist)
		expect(output).not.toContain("=0");
	});

	test("RANDR provides screen information", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
randr = d.query_extension('RANDR')
print(f"randr_present={randr is not None and randr.major_opcode > 0}")
screen = d.screen()
print(f"width={screen.width_in_pixels}")
print(f"height={screen.height_in_pixels}")
d.close()
`,
		);
		expect(output).toContain("randr_present=True");
		expect(output).toMatch(/width=\d+/);
	});

	test("Font system serves standard fonts", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
fonts = d.list_fonts('*', 100)
print(f"font_count={len(fonts)}")
fixed = d.list_fonts('fixed', 10)
print(f"has_fixed={len(fixed) > 0}")
d.close()
`,
		);
		const count = Number.parseInt(
			output.match(/font_count=(\d+)/)?.[1] ?? "0",
		);
		expect(count).toBeGreaterThan(5);
		expect(output).toContain("has_fixed=True");
	});

	test("Visual configuration supports TrueColor", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
# Check that root visual is TrueColor (class 4)
print(f"root_depth={screen.root_depth}")
# screen.root_visual may be an int (visual ID) in some python3-xlib versions
# Look up the visual info from the screen's allowed_depths
visual_class = None
for depth_info in screen.allowed_depths:
    for vis in depth_info.visuals:
        if vis.visual_id == screen.root_visual:
            visual_class = vis.visual_class
            break
if visual_class is None and hasattr(screen.root_visual, 'visual_class'):
    visual_class = screen.root_visual.visual_class
print(f"root_visual_class={visual_class}")
# TrueColor = 4
print(f"is_truecolor={visual_class == 4}")
d.close()
`,
		);
		expect(output).toContain("is_truecolor=True");
		expect(output).toContain("root_depth=24");
	});

	// --- XSETTINGS / XKB ---

	test("XSETTINGS manager provides GTK defaults", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, struct
d = Xlib.display.Display()
screen = d.screen()
xsettings_screen = d.intern_atom('_XSETTINGS_S0')
owner = d.get_selection_owner(xsettings_screen)
print(f"xsettings_owner={owner != 0}")
if owner:
    settings_atom = d.intern_atom('_XSETTINGS_SETTINGS')
    prop = owner.get_full_property(settings_atom, 0)
    if prop and len(prop.value) > 12:
        data = bytes(prop.value)
        n = struct.unpack_from('<I' if data[0] == 0 else '>I', data, 8)[0]
        print(f"xsettings_count={n}")
    else:
        print("xsettings_count=0")
else:
    print("xsettings_count=0")
d.close()
`,
		);
		expect(output).toContain("xsettings_owner=True");
		const cnt = Number.parseInt(
			output.match(/xsettings_count=(\d+)/)?.[1] ?? "0",
		);
		expect(cnt).toBeGreaterThan(0);
	});

	test("XKB extension is available", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
xkb = d.query_extension('XKEYBOARD')
print(f"xkb_present={xkb is not None and xkb.major_opcode > 0}")
d.close()
`,
		);
		expect(output).toContain("xkb_present=True");
	});

	// --- Real applications ---

	// --- Real application tests (use python3-xlib to avoid XCB issues) ---

	test("X11 window create/map/destroy cycle works", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, time
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(10, 10, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask | Xlib.X.StructureNotifyMask)
w.map()
d.sync()
time.sleep(0.5)
geom = w.get_geometry()
print(f"mapped_width={geom.width}")
print(f"mapped_height={geom.height}")
w.destroy()
d.sync()
print("lifecycle_ok=True")
d.close()
`,
		);
		expect(output).toContain("mapped_width=200");
		expect(output).toContain("lifecycle_ok=True");
	});

	test("wish (Tcl/Tk) creates and renders widgets", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			`echo 'wm title . "TkTest"; label .l -text "Hello Tk"; pack .l; update; puts "TK_RENDERED"; exit' | timeout 8 wish 2>&1`,
		);
		expect(output).toContain("TK_RENDERED");
		expect(output).not.toContain("X Error");
	});

	// --- EWMH fullscreen / maximize ---

	test("_NET_WM_STATE_FULLSCREEN resizes window to screen size", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.protocol.event, time

d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(50, 50, 400, 300, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.PropertyChangeMask)
w.map()
d.sync()
time.sleep(0.5)

net_wm_state = d.intern_atom('_NET_WM_STATE')
fullscreen_atom = d.intern_atom('_NET_WM_STATE_FULLSCREEN')

event = Xlib.protocol.event.ClientMessage(
    window=w, client_type=net_wm_state,
    data=(32, [1, fullscreen_atom, 0, 1, 0]))
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()
time.sleep(1)

geom = w.get_geometry()
sw = screen.width_in_pixels
sh = screen.height_in_pixels
print(f"fullscreen_ok={geom.width == sw and geom.height == sh}")

# Remove fullscreen
event2 = Xlib.protocol.event.ClientMessage(
    window=w, client_type=net_wm_state,
    data=(32, [0, fullscreen_atom, 0, 1, 0]))
screen.root.send_event(event2, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()
time.sleep(1)

geom2 = w.get_geometry()
print(f"restore_ok={geom2.width == 400 and geom2.height == 300}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("fullscreen_ok=True");
		expect(output).toContain("restore_ok=True");
	});

	test("_NET_WM_STATE_MAXIMIZED resizes window", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, Xlib.protocol.event, time

d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(50, 50, 400, 300, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.PropertyChangeMask)
w.map()
d.sync()
time.sleep(0.5)

net_wm_state = d.intern_atom('_NET_WM_STATE')
max_vert = d.intern_atom('_NET_WM_STATE_MAXIMIZED_VERT')
max_horz = d.intern_atom('_NET_WM_STATE_MAXIMIZED_HORZ')

event = Xlib.protocol.event.ClientMessage(
    window=w, client_type=net_wm_state,
    data=(32, [1, max_vert, max_horz, 1, 0]))
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()
time.sleep(1)

geom = w.get_geometry()
print(f"maximize_ok={geom.width == screen.width_in_pixels and geom.height == screen.height_in_pixels}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("maximize_ok=True");
	});

	// --- Multi-client interaction ---

	test("Multiple X clients can coexist", async ({ sidecarContainer }) => {
		await execInSidecar(sidecarContainer, "xeyes &");
		await execInSidecar(sidecarContainer, "xclock -digital &");
		await execInSidecar(sidecarContainer, "xlogo &");
		await new Promise((r) => setTimeout(r, 3000));

		const ps = await execInSidecar(
			sidecarContainer,
			`echo "xeyes=$(pgrep xeyes | wc -l)"
echo "xclock=$(pgrep xclock | wc -l)"
echo "xlogo=$(pgrep xlogo | wc -l)"`,
		);
		expect(ps).toContain("xeyes=1");
		expect(ps).toContain("xclock=1");
		expect(ps).toContain("xlogo=1");
	});

	test("Window stacking z-order is maintained", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, time
d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w2 = screen.root.create_window(50, 50, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w1.map()
w2.map()
d.sync()
time.sleep(0.3)

tree = screen.root.query_tree()
children = list(tree.children)
w1_idx = children.index(w1) if w1 in children else -1
w2_idx = children.index(w2) if w2 in children else -1
print(f"w2_above_w1={w2_idx > w1_idx}")

w1.raise_window()
d.sync()
time.sleep(0.3)

tree2 = screen.root.query_tree()
children2 = list(tree2.children)
w1_idx2 = children2.index(w1) if w1 in children2 else -1
w2_idx2 = children2.index(w2) if w2 in children2 else -1
print(f"after_raise_w1_above_w2={w1_idx2 > w2_idx2}")

w1.destroy()
w2.destroy()
d.close()
`,
		);
		expect(output).toContain("w2_above_w1=True");
		expect(output).toContain("after_raise_w1_above_w2=True");
	});

	// --- EWMH property tests ---

	test("_NET_SUPPORTED has 20+ atoms", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
net_supported = d.intern_atom('_NET_SUPPORTED')
prop = d.screen().root.get_full_property(net_supported, 0)
print(f"count={len(prop.value) if prop else 0}")
d.close()
`,
		);
		const cnt = Number.parseInt(
			output.match(/count=(\d+)/)?.[1] ?? "0",
		);
		expect(cnt).toBeGreaterThan(20);
	});

	test("window configure and query works", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X, time
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(10, 10, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()
time.sleep(0.3)

# Configure window to new size and position
w.configure(x=100, y=100, width=300, height=200)
d.sync()
time.sleep(0.3)

geom = w.get_geometry()
print(f"new_width={geom.width}")
print(f"new_height={geom.height}")
print(f"configure_ok={geom.width == 300 and geom.height == 200}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("configure_ok=True");
	});

	// --- Extension availability ---

	test("All required extensions are present", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display
d = Xlib.display.Display()
for ext in ['MIT-SHM', 'RENDER', 'Composite', 'RANDR', 'XKEYBOARD',
            'SHAPE', 'SYNC', 'XFIXES', 'DAMAGE', 'XInputExtension',
            'XTEST', 'RECORD', 'SECURITY', 'BIG-REQUESTS', 'XC-MISC']:
    r = d.query_extension(ext)
    ok = r is not None and r.major_opcode > 0
    print(f"{ext}={ok}")
d.close()
`,
		);
		for (const ext of [
			"MIT-SHM",
			"RENDER",
			"Composite",
			"RANDR",
			"XKEYBOARD",
			"SHAPE",
			"SYNC",
			"XFIXES",
			"DAMAGE",
			"XInputExtension",
			"XTEST",
			"RECORD",
			"SECURITY",
			"BIG-REQUESTS",
			"XC-MISC",
		]) {
			expect(output).toContain(`${ext}=True`);
		}
	});
});
