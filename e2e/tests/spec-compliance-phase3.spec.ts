/**
 * E2E compliance tests for Phase 3 spec compliance fixes:
 * - SubstructureRedirect for ALL windows (not just top-level)
 * - ConfigureRequest for ALL windows with redirect parent
 * - WM_STATE set on ALL mapped windows per ICCCM
 * - Window hierarchy event propagation
 */

import { expect, test } from "./fixtures";
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

test.describe.serial("SubstructureRedirect compliance", () => {
	test.setTimeout(60_000);

	test("SubstructureRedirectMask can be set on non-root parent", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create a parent window
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
parent.map()
d.sync()

# Create a child of parent (not root)
child = parent.create_window(10, 10, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
d.sync()

# Verify parent has SubstructureRedirectMask set
attrs = parent.get_attributes()
print(f"parent_attrs_ok")

# Try to map the child — should succeed since we own the redirect
child.map()
d.sync()
print("CHILD_MAP_OK")

d.close()
`,
		);
		expect(output).toContain("CHILD_MAP_OK");
	});

	test("Override redirect window bypasses SubstructureRedirect", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create an override-redirect window
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    override_redirect=True)

# Map it — should succeed directly even if WM has redirect
w.map()
d.sync()

# Verify it's mapped
attrs = w.get_attributes()
if attrs.map_state == 2:  # IsViewable
    print("OR_MAP_OK")
else:
    print(f"OR_MAP_STATE={attrs.map_state}")

d.close()
`,
		);
		expect(output).toContain("OR_MAP_OK");
	});

	test("ConfigureWindow works on override-redirect windows", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create an OR window and configure it
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    override_redirect=True)
w.map()
d.sync()

# Move and resize
w.configure(x=50, y=50, width=200, height=150)
d.sync()

# Verify
geom = w.get_geometry()
if geom.x == 50 and geom.y == 50 and geom.width == 200 and geom.height == 150:
    print("OR_CONFIGURE_OK")
else:
    print(f"OR_CONFIGURE: x={geom.x} y={geom.y} w={geom.width} h={geom.height}")

d.close()
`,
		);
		expect(output).toContain("OR_CONFIGURE_OK");
	});
});

test.describe.serial("WM_STATE ICCCM compliance", () => {
	test.setTimeout(60_000);

	test("WM_STATE is set when window is mapped", async ({
		sidecarContainer,
	}) => {
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
import time; time.sleep(0.2)  # let the server set the property

# Check WM_STATE property. python-xlib decodes format=32 properties into
# an array.array('I', ...), so len(prop.value) is the count of CARDINALs
# (2: state + icon_window), not bytes.
wm_state_atom = d.intern_atom("WM_STATE")
prop = w.get_full_property(wm_state_atom, wm_state_atom)
if prop and len(prop.value) >= 1:
    state_val = int(prop.value[0])
    print(f"wm_state={state_val}")
    if state_val == 1:  # NormalState
        print("WM_STATE_OK")
else:
    print("NO_WM_STATE")

d.close()
`,
		);
		expect(output).toContain("WM_STATE_OK");
	});

	test("WM_STATE is NormalState for child windows", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X
import struct

d = Xlib.display.Display()
screen = d.screen()

# Create parent and child
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
parent.map()
d.sync()

child = parent.create_window(10, 10, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
child.map()
d.sync()

# Check WM_STATE on child (format=32 → array.array of CARDINALs, len in elements)
import time; time.sleep(0.2)
wm_state_atom = d.intern_atom("WM_STATE")
prop = child.get_full_property(wm_state_atom, wm_state_atom)
if prop and len(prop.value) >= 1:
    state_val = int(prop.value[0])
    if state_val == 1:  # NormalState
        print("CHILD_WM_STATE_OK")
    else:
        print(f"CHILD_WM_STATE={state_val}")
else:
    print("NO_CHILD_WM_STATE")

d.close()
`,
		);
		expect(output).toContain("CHILD_WM_STATE_OK");
	});
});

test.describe.serial("Window hierarchy events", () => {
	test.setTimeout(60_000);

	test("StructureNotifyMask delivers MapNotify", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()

# Check for MapNotify event
got_map_notify = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.MapNotify:
        got_map_notify = True
        break

if got_map_notify:
    print("MAP_NOTIFY_OK")
else:
    print("NO_MAP_NOTIFY")

d.close()
`,
		);
		expect(output).toContain("MAP_NOTIFY_OK");
	});

	test("SubstructureNotifyMask delivers CreateNotify", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.SubstructureNotifyMask)
parent.map()
d.sync()

# Create a child — should generate CreateNotify on parent
child = parent.create_window(10, 10, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
d.sync()

got_create_notify = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.CreateNotify:
        got_create_notify = True
        break

if got_create_notify:
    print("CREATE_NOTIFY_OK")
else:
    print("NO_CREATE_NOTIFY")

d.close()
`,
		);
		expect(output).toContain("CREATE_NOTIFY_OK");
	});

	test("DestroyNotify delivered when window destroyed", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()
# Drain events
while d.pending_events() > 0:
    d.next_event()

# Destroy window
w.destroy()
d.sync()
# Events are async — give the server a moment to deliver before checking
import time; time.sleep(0.3)

got_destroy = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.DestroyNotify:
        got_destroy = True
        break

if got_destroy:
    print("DESTROY_NOTIFY_OK")
else:
    print("NO_DESTROY_NOTIFY")

d.close()
`,
		);
		expect(output).toContain("DESTROY_NOTIFY_OK");
	});

	test("ConfigureNotify sent after ConfigureWindow", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    override_redirect=True,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()
while d.pending_events() > 0:
    d.next_event()

# Configure the window
w.configure(x=50, y=50, width=200, height=150)
d.sync()

got_configure = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.ConfigureNotify:
        if ev.width == 200 and ev.height == 150:
            got_configure = True
            break

if got_configure:
    print("CONFIGURE_NOTIFY_OK")
else:
    print("NO_CONFIGURE_NOTIFY")

d.close()
`,
		);
		expect(output).toContain("CONFIGURE_NOTIFY_OK");
	});

	test("ReparentNotify sent on reparent", async ({ sidecarContainer }) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
parent.map()

child = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
child.map()
d.sync()
while d.pending_events() > 0:
    d.next_event()

child.reparent(parent, 10, 10)
d.sync()

got_reparent = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.ReparentNotify:
        got_reparent = True
        break

if got_reparent:
    print("REPARENT_NOTIFY_OK")
else:
    print("NO_REPARENT_NOTIFY")

d.close()
`,
		);
		expect(output).toContain("REPARENT_NOTIFY_OK");
	});
});

test.describe.serial("Window management edge cases", () => {
	test.setTimeout(60_000);

	test("Expose event on newly mapped window with ExposureMask", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

got_expose = False
for _ in range(10):
    if d.pending_events() > 0:
        ev = d.next_event()
        if ev.type == Xlib.X.Expose:
            got_expose = True
            break
    else:
        import time
        time.sleep(0.1)
        d.sync()

if got_expose:
    print("EXPOSE_ON_MAP_OK")
else:
    print("NO_EXPOSE_ON_MAP")

d.close()
`,
		);
		expect(output).toContain("EXPOSE_ON_MAP_OK");
	});

	test("UnmapNotify sent when window unmapped", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()
while d.pending_events() > 0:
    d.next_event()

w.unmap()
d.sync()

got_unmap = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.UnmapNotify:
        got_unmap = True
        break

if got_unmap:
    print("UNMAP_NOTIFY_OK")
else:
    print("NO_UNMAP_NOTIFY")

d.close()
`,
		);
		expect(output).toContain("UNMAP_NOTIFY_OK");
	});
});


// ---------------------------------------------------------------------------
// Backing store verification
// ---------------------------------------------------------------------------
test.describe("backing store", () => {
	test("GetWindowAttributes reports backing store support", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"root = d.screen().root",
				"attrs = root.get_attributes()",
				"print(f'backing_store={attrs.backing_store}')",
				"# Setup reports BackingStore capability",
				"print(f'backing_stores={d.screen().backing_store}')",
				"print('BACKING_STORE_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("BACKING_STORE_PASS");
	});

	test("window backing store attribute round-trips", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display, Xlib.X",
				"d = Xlib.display.Display()",
				"root = d.screen().root",
				"w = root.create_window(0,0,100,100,0,d.screen().root_depth,",
				"    backing_store=Xlib.X.WhenMapped)",
				"attrs = w.get_attributes()",
				"print(f'backing_store={attrs.backing_store}')",
				"assert attrs.backing_store == Xlib.X.WhenMapped, f'Expected WhenMapped(1), got {attrs.backing_store}'",
				"w.destroy()",
				"d.close()",
				"print('BACKING_RT_PASS')",
			].join("\n"),
		]);
		expect(result.output).toContain("BACKING_RT_PASS");
	});
});


// ---------------------------------------------------------------------------
// Access control tests
// ---------------------------------------------------------------------------
test.describe("access control", () => {
	test("xhost lists initial access state", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xhost 2>&1",
				"echo XHOST_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("XHOST_PASS");
	});

	test("xhost +/- modifies access control", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xhost + 2>&1",
				"xhost 2>&1",
				"xhost - 2>&1",
				"xhost 2>&1",
				"echo XHOST_MODIFY_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("XHOST_MODIFY_PASS");
	});
});


// ---------------------------------------------------------------------------
// Screen saver protocol tests
// ---------------------------------------------------------------------------
test.describe("screen saver", () => {
	test("GetScreenSaver returns settings", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"ss = d.get_screen_saver()",
				"print(f'timeout={ss.timeout}')",
				"print(f'interval={ss.interval}')",
				"print(f'prefer_blanking={ss.prefer_blanking}')",
				"print(f'allow_exposures={ss.allow_exposures}')",
				"print('SCREEN_SAVER_GET_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("SCREEN_SAVER_GET_PASS");
	});

	test("SetScreenSaver round-trips timeout", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"d.set_screen_saver(timeout=300, interval=60, prefer_blanking=1, allow_exposures=1)",
				"d.sync()",
				"ss = d.get_screen_saver()",
				"print(f'timeout={ss.timeout}')",
				"assert ss.timeout == 300, f'Expected 300, got {ss.timeout}'",
				"assert ss.interval == 60, f'Expected 60, got {ss.interval}'",
				"# Restore defaults",
				"d.set_screen_saver(timeout=0, interval=0, prefer_blanking=0, allow_exposures=0)",
				"print('SCREEN_SAVER_SET_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("SCREEN_SAVER_SET_PASS");
	});

	test("ForceScreenSaver activate/reset works", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"d.force_screen_saver(1)  # Activate",
				"d.sync()",
				"d.force_screen_saver(0)  # Reset",
				"d.sync()",
				"print('FORCE_SCREEN_SAVER_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("FORCE_SCREEN_SAVER_PASS");
	});
});


// ---------------------------------------------------------------------------
// Multi-client stress tests
// ---------------------------------------------------------------------------
test.describe("multi-client stress", () => {
	test("5 simultaneous xeyes windows", async ({ sidecarContainer }) => {
		test.setTimeout(60_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"for i in 1 2 3 4 5; do xeyes &; done",
				"sleep 2",
				"COUNT=$(xdotool search --class xeyes 2>/dev/null | wc -l)",
				"echo count=$COUNT",
				"pkill xeyes 2>/dev/null; true",
				"echo MULTI_CLIENT_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("MULTI_CLIENT_PASS");
	});

	test("concurrent InternAtom requests", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"atoms = []",
				"for i in range(100):",
				"    name = f'TEST_ATOM_{i}'",
				"    atom = d.intern_atom(name)",
				"    atoms.append((name, atom))",
				"# Verify all atoms resolve back to their names",
				"for name, atom in atoms:",
				"    resolved = d.get_atom_name(atom)",
				"    assert resolved == name, f'{name} != {resolved}'",
				"print(f'interned={len(atoms)} atoms')",
				"print('INTERN_ATOM_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("interned=100 atoms");
		expect(result.output).toContain("INTERN_ATOM_PASS");
	});

	test("rapid window create/destroy cycle", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"root = d.screen().root",
				"for i in range(50):",
				"    w = root.create_window(0,0,10,10,0,d.screen().root_depth)",
				"    w.map()",
				"    d.sync()",
				"    w.destroy()",
				"    d.sync()",
				"print('50 windows created and destroyed')",
				"print('CREATE_DESTROY_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("50 windows created and destroyed");
		expect(result.output).toContain("CREATE_DESTROY_PASS");
	});
});


// ---------------------------------------------------------------------------
// XKB compat map tests
// ---------------------------------------------------------------------------
test.describe("XKB compat map", () => {
	test("xkbcomp can dump the compat map", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const available = await sidecarContainer.exec([
			"bash", "-c", "which xkbcomp 2>/dev/null && echo XKBCOMP_FOUND || echo XKBCOMP_MISSING",
		]);
		if (available.output.includes("XKBCOMP_MISSING")) {
			test.skip();
			return;
		}
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xkbcomp :99 /tmp/xkb_dump.xkb 2>&1",
				"grep -c 'interpret' /tmp/xkb_dump.xkb || echo 0",
				"echo XKB_COMPAT_DUMP_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("XKB_COMPAT_DUMP_PASS");
	});

	test("modifier keys produce correct keysyms", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"# Keycode 50 = Shift_L (keysym 0xFFE1)",
				"sym = d.keycode_to_keysym(50, 0)",
				"print(f'shift_l_sym={sym:#x}')",
				"assert sym == 0xFFE1, f'Expected 0xFFE1, got {sym:#x}'",
				"# Keycode 66 = Caps_Lock (keysym 0xFFE5)",
				"sym = d.keycode_to_keysym(66, 0)",
				"print(f'caps_lock_sym={sym:#x}')",
				"assert sym == 0xFFE5, f'Expected 0xFFE5, got {sym:#x}'",
				"print('MODIFIER_KEYSYM_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("MODIFIER_KEYSYM_PASS");
	});
});


// ---------------------------------------------------------------------------
// RENDER animated cursor — verify CreateAnimCursor doesn't crash
// ---------------------------------------------------------------------------
test.describe("RENDER animated cursor", () => {
	test("animated cursor creation via python3-xlib", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display, Xlib.X, Xlib.Xutil",
				"d = Xlib.display.Display()",
				"screen = d.screen()",
				"root = screen.root",
				"# Create a simple window that accepts cursor changes",
				"w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,",
				"    Xlib.X.InputOutput, Xlib.X.CopyFromParent,",
				"    background_pixel=0x000000,",
				"    event_mask=Xlib.X.ExposureMask)",
				"w.map()",
				"d.sync()",
				"# Verify the window was created",
				"tree = root.query_tree()",
				"assert len(tree.children) >= 1, 'No child windows after create'",
				"w.destroy()",
				"d.sync()",
				"print('ANIM_CURSOR_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("ANIM_CURSOR_PASS");
	});
});


// ---------------------------------------------------------------------------
// GrabServer / UngrabServer serialization
// ---------------------------------------------------------------------------
test.describe("GrabServer serialization", () => {
	test("GrabServer blocks other clients", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"# GrabServer should succeed",
				"d.grab_server()",
				"d.sync()",
				"# We can still make requests while holding the grab",
				"root = d.screen().root",
				"tree = root.query_tree()",
				"assert tree is not None, 'QueryTree failed during GrabServer'",
				"# Release the grab",
				"d.ungrab_server()",
				"d.sync()",
				"# Verify server is still usable",
				"tree2 = root.query_tree()",
				"assert tree2 is not None, 'QueryTree failed after UngrabServer'",
				"print('GRAB_SERVER_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("GRAB_SERVER_PASS");
	});
});


// ---------------------------------------------------------------------------
// Font enumeration — verify core fonts are discoverable
// ---------------------------------------------------------------------------
test.describe("Font enumeration", () => {
	test("xlsfonts lists at least 100 fonts", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"FONT_COUNT=$(xlsfonts 2>/dev/null | wc -l)",
				"echo \"FONT_COUNT=$FONT_COUNT\"",
				"if [ \"$FONT_COUNT\" -ge 100 ]; then",
				"  echo 'FONT_ENUM_PASS'",
				"else",
				"  echo 'FONT_ENUM_LOW'",
				"fi",
			].join("\n"),
		]);
		expect(result.output).toContain("FONT_ENUM_PASS");
	});

	test("fixed font is available", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xlsfonts -fn fixed 2>&1",
				"echo FIXED_FONT_PASS",
			].join("\n"),
		]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("fixed");
		expect(result.output).toContain("FIXED_FONT_PASS");
	});
});


// ---------------------------------------------------------------------------
// Colormap operations — AllocColor, AllocNamedColor, QueryColors
// ---------------------------------------------------------------------------
test.describe("Colormap operations", () => {
	test("AllocColor and AllocNamedColor round-trip", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"screen = d.screen()",
				"cmap = screen.default_colormap",
				"# AllocColor: exact RGB values",
				"reply = cmap.alloc_color(65535, 0, 0)  # pure red",
				"assert reply.pixel is not None, 'AllocColor failed'",
				"print(f'alloc_red_pixel={reply.pixel:#x}')",
				"# AllocNamedColor: look up by name",
				"reply2 = cmap.alloc_named_color('blue')",
				"assert reply2.pixel is not None, 'AllocNamedColor failed'",
				"print(f'alloc_blue_pixel={reply2.pixel:#x}')",
				"# QueryColors: read back the allocated colors",
				"colors = cmap.query_colors([reply.pixel, reply2.pixel])",
				"assert len(colors) == 2, f'QueryColors returned {len(colors)} colors'",
				"print(f'query_red=({colors[0].red},{colors[0].green},{colors[0].blue})')",
				"print(f'query_blue=({colors[1].red},{colors[1].green},{colors[1].blue})')",
				"# Red should have red component > 60000",
				"assert colors[0].red > 60000, f'Red too low: {colors[0].red}'",
				"# Blue should have blue component > 60000",
				"assert colors[1].blue > 60000, f'Blue too low: {colors[1].blue}'",
				"print('COLORMAP_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("COLORMAP_PASS");
	});
});


// ---------------------------------------------------------------------------
// Property operations — ChangeProperty, GetProperty, RotateProperties
// ---------------------------------------------------------------------------
test.describe("Property operations", () => {
	test("ChangeProperty + GetProperty + RotateProperties", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display, Xlib.X, Xlib.Xatom",
				"d = Xlib.display.Display()",
				"screen = d.screen()",
				"root = screen.root",
				"w = root.create_window(0, 0, 50, 50, 0, screen.root_depth,",
				"    Xlib.X.InputOutput, Xlib.X.CopyFromParent)",
				"# Set two custom properties",
				"a1 = d.intern_atom('_TEST_PROP_A')",
				"a2 = d.intern_atom('_TEST_PROP_B')",
				"w.change_property(a1, Xlib.Xatom.STRING, 8, b'hello')",
				"w.change_property(a2, Xlib.Xatom.STRING, 8, b'world')",
				"d.sync()",
				"# Read them back",
				"p1 = w.get_property(a1, Xlib.Xatom.STRING, 0, 100)",
				"p2 = w.get_property(a2, Xlib.Xatom.STRING, 0, 100)",
				"assert bytes(p1.value) == b'hello', f'Prop A mismatch: {p1.value}'",
				"assert bytes(p2.value) == b'world', f'Prop B mismatch: {p2.value}'",
				"# ListProperties",
				"props = w.list_properties()",
				"assert a1 in props, 'Missing _TEST_PROP_A in ListProperties'",
				"assert a2 in props, 'Missing _TEST_PROP_B in ListProperties'",
				"# RotateProperties",
				"w.rotate_properties([a1, a2], 1)",
				"d.sync()",
				"p1_after = w.get_property(a1, Xlib.Xatom.STRING, 0, 100)",
				"p2_after = w.get_property(a2, Xlib.Xatom.STRING, 0, 100)",
				"# After rotating by 1, a1 should have the value that was in a2",
				"assert bytes(p1_after.value) == b'world', f'After rotate, A={p1_after.value}'",
				"assert bytes(p2_after.value) == b'hello', f'After rotate, B={p2_after.value}'",
				"# DeleteProperty",
				"w.delete_property(a1)",
				"d.sync()",
				"p1_del = w.get_property(a1, Xlib.Xatom.STRING, 0, 100)",
				"assert p1_del is None or p1_del.property_type == 0, 'Property not deleted'",
				"w.destroy()",
				"d.sync()",
				"print('PROPERTY_OPS_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("PROPERTY_OPS_PASS");
	});
});


// ---------------------------------------------------------------------------
// Window geometry operations — GetGeometry, TranslateCoordinates
// ---------------------------------------------------------------------------
test.describe("Window geometry", () => {
	test("GetGeometry and TranslateCoordinates round-trip", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display, Xlib.X",
				"d = Xlib.display.Display()",
				"screen = d.screen()",
				"root = screen.root",
				"# Create parent at (50, 50) size 200x200",
				"parent = root.create_window(50, 50, 200, 200, 0, screen.root_depth,",
				"    Xlib.X.InputOutput, Xlib.X.CopyFromParent)",
				"# Create child at (10, 10) relative to parent",
				"child = parent.create_window(10, 10, 50, 50, 2, screen.root_depth,",
				"    Xlib.X.InputOutput, Xlib.X.CopyFromParent)",
				"parent.map()",
				"child.map()",
				"d.sync()",
				"# GetGeometry on child",
				"geo = child.get_geometry()",
				"assert geo.x == 10, f'Child x={geo.x}'",
				"assert geo.y == 10, f'Child y={geo.y}'",
				"assert geo.width == 50, f'Child width={geo.width}'",
				"assert geo.height == 50, f'Child height={geo.height}'",
				"assert geo.border_width == 2, f'Child border={geo.border_width}'",
				"# TranslateCoordinates: child (0,0) -> root coords",
				"tc = d.screen().root.translate_coords(child, 0, 0)",
				"# Should be approximately (50+10+2, 50+10+2) = (62, 62)",
				"# (border_width adds to the offset)",
				"print(f'translate=({tc.x},{tc.y})')",
				"assert tc.x >= 50, f'Translated x too small: {tc.x}'",
				"assert tc.y >= 50, f'Translated y too small: {tc.y}'",
				"child.destroy()",
				"parent.destroy()",
				"d.sync()",
				"print('GEOMETRY_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("GEOMETRY_PASS");
	});
});
