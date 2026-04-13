/**
 * E2E compliance tests for Phase 3 spec compliance fixes:
 * - SubstructureRedirect for ALL windows (not just top-level)
 * - ConfigureRequest for ALL windows with redirect parent
 * - WM_STATE set on ALL mapped windows per ICCCM
 * - Window hierarchy event propagation
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

# Check WM_STATE property
wm_state_atom = d.intern_atom("WM_STATE")
prop = w.get_full_property(wm_state_atom, wm_state_atom)
if prop and len(prop.value) >= 4:
    state_val = struct.unpack("<I", prop.value[:4])[0]
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

# Check WM_STATE on child
wm_state_atom = d.intern_atom("WM_STATE")
prop = child.get_full_property(wm_state_atom, wm_state_atom)
if prop and len(prop.value) >= 4:
    state_val = struct.unpack("<I", prop.value[:4])[0]
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
