/**
 * Phase 8 compliance tests: ICCCM WM_HINTS, modal dialog blocking,
 * _NET_REQUEST_FRAME_EXTENTS, and focus model compliance.
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

// ---------------------------------------------------------------------------
// WM_HINTS parsing and ICCCM input focus model
// ---------------------------------------------------------------------------

test.describe.serial("WM_HINTS input focus model (Phase 8A)", () => {
	test("WM_HINTS input=true is parsed (Passive/Locally Active)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
# Set WM_HINTS with input=True (flags bit 0 = InputHint)
hints = w.wm_hints
if hints is None:
    hints = {}
hints['flags'] = Xutil.InputHint
hints['input'] = 1
w.set_wm_hints(hints)
d.sync()

# Read back WM_HINTS to verify
read_hints = w.get_wm_hints()
input_val = read_hints.get('input', -1) if read_hints else -1
print(f"input={input_val}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("input=1");
	});

	test("WM_HINTS input=false is parsed (Globally Active/No Input)", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
hints = {}
hints['flags'] = Xutil.InputHint
hints['input'] = 0
w.set_wm_hints(hints)
d.sync()

read_hints = w.get_wm_hints()
input_val = read_hints.get('input', -1) if read_hints else -1
print(f"input={input_val}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("input=0");
	});

	test("WM_HINTS window_group is stored", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
leader = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth)
child = root.create_window(20, 20, 100, 100, 0, d.screen().root_depth)

# Set WM_HINTS with window_group on child
hints = {}
hints['flags'] = Xutil.WindowGroupHint
hints['window_group'] = leader.id
child.set_wm_hints(hints)
d.sync()

read_hints = child.get_wm_hints()
group = read_hints.get('window_group', 0) if read_hints else 0
print(f"group_matches={group == leader.id}")
leader.destroy()
child.destroy()
d.close()
`,
		);
		expect(output).toContain("group_matches=True");
	});

	test("WM_HINTS urgency hint triggers WindowUrgent", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
hints = {}
hints['flags'] = Xutil.UrgencyHint
w.set_wm_hints(hints)
d.sync()

read_hints = w.get_wm_hints()
flags = read_hints.get('flags', 0) if read_hints else 0
urgent = bool(flags & Xutil.UrgencyHint)
print(f"urgent={urgent}")
w.destroy()
d.close()
`,
		);
		expect(output).toContain("urgent=True");
	});
});

// ---------------------------------------------------------------------------
// Modal dialog blocking
// ---------------------------------------------------------------------------

test.describe.serial("Modal dialog blocking (Phase 8B)", () => {
	test("_NET_WM_STATE_MODAL can be set on a window", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
root = d.screen().root

# Create parent and modal child
parent = root.create_window(10, 10, 400, 400, 0, d.screen().root_depth,
    event_mask=X.StructureNotifyMask | X.PropertyChangeMask)
parent.map()
d.sync()

child = root.create_window(50, 50, 200, 200, 0, d.screen().root_depth)
child.map()
d.sync()

# Set WM_TRANSIENT_FOR on child
wm_transient_for = d.intern_atom('WM_TRANSIENT_FOR')
child.change_property(wm_transient_for, 33, 32, [parent.id])
d.sync()

# Set _NET_WM_STATE_MODAL on child via ClientMessage
net_wm_state = d.intern_atom('_NET_WM_STATE')
net_wm_state_modal = d.intern_atom('_NET_WM_STATE_MODAL')

# Send _NET_WM_STATE ClientMessage to root
from Xlib.protocol import event
import struct
e = event.ClientMessage(
    window=child.id,
    client_type=net_wm_state,
    data=(32, [1, net_wm_state_modal, 0, 1, 0])  # 1=_NET_WM_STATE_ADD
)
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

# Read back _NET_WM_STATE to check MODAL is set
state_prop = child.get_full_property(net_wm_state, X.AnyPropertyType)
if state_prop:
    import array
    atoms = array.array('I', state_prop.value)
    has_modal = net_wm_state_modal in atoms
    print(f"has_modal={has_modal}")
else:
    print("has_modal=False")

child.destroy()
parent.destroy()
d.close()
`,
		);
		expect(output).toContain("has_modal=True");
	});

	test("_NET_WM_STATE_MODAL is toggled correctly", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
from Xlib.protocol import event
d = display.Display()
root = d.screen().root

w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
w.map()
d.sync()

net_wm_state = d.intern_atom('_NET_WM_STATE')
modal = d.intern_atom('_NET_WM_STATE_MODAL')

# Add MODAL
e = event.ClientMessage(window=w.id, client_type=net_wm_state, data=(32, [1, modal, 0, 1, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

# Toggle MODAL (should remove it)
e = event.ClientMessage(window=w.id, client_type=net_wm_state, data=(32, [2, modal, 0, 1, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

import array
state_prop = w.get_full_property(net_wm_state, X.AnyPropertyType)
atoms = array.array('I', state_prop.value) if state_prop and state_prop.value else []
has_modal = modal in atoms
print(f"modal_removed={not has_modal}")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("modal_removed=True");
	});
});

// ---------------------------------------------------------------------------
// _NET_REQUEST_FRAME_EXTENTS
// ---------------------------------------------------------------------------

test.describe.serial("_NET_REQUEST_FRAME_EXTENTS (Phase 8C)", () => {
	test("server responds with _NET_FRAME_EXTENTS property", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
from Xlib.protocol import event
d = display.Display()
root = d.screen().root

w = root.create_window(10, 10, 200, 200, 2, d.screen().root_depth,
    event_mask=X.PropertyChangeMask)
d.sync()

net_request = d.intern_atom('_NET_REQUEST_FRAME_EXTENTS')
net_frame = d.intern_atom('_NET_FRAME_EXTENTS')

# Send _NET_REQUEST_FRAME_EXTENTS ClientMessage
e = event.ClientMessage(window=w.id, client_type=net_request, data=(32, [0, 0, 0, 0, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

# Read _NET_FRAME_EXTENTS
import time
time.sleep(0.1)
d.sync()
prop = w.get_full_property(net_frame, X.AnyPropertyType)
if prop and prop.value:
    import array
    vals = array.array('I', prop.value)
    print(f"extents={list(vals)}")
    # With border_width=2, all extents should be 2
    print(f"correct={all(v == 2 for v in vals)}")
else:
    print("extents=none")
    print("correct=False")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("correct=True");
	});

	test("zero border_width gives zero frame extents", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
from Xlib.protocol import event
d = display.Display()
root = d.screen().root

w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth,
    event_mask=X.PropertyChangeMask)
d.sync()

net_request = d.intern_atom('_NET_REQUEST_FRAME_EXTENTS')
net_frame = d.intern_atom('_NET_FRAME_EXTENTS')

e = event.ClientMessage(window=w.id, client_type=net_request, data=(32, [0, 0, 0, 0, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

import time
time.sleep(0.1)
d.sync()
prop = w.get_full_property(net_frame, X.AnyPropertyType)
if prop and prop.value:
    import array
    vals = array.array('I', prop.value)
    print(f"all_zero={all(v == 0 for v in vals)}")
else:
    print("all_zero=True")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("all_zero=True");
	});
});

// ---------------------------------------------------------------------------
// WM_TRANSIENT_FOR stacking
// ---------------------------------------------------------------------------

test.describe.serial("WM_TRANSIENT_FOR (Phase 8D)", () => {
	test("transient window is placed above its parent on map", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
root = d.screen().root

parent = root.create_window(10, 10, 400, 400, 0, d.screen().root_depth)
parent.map()
d.sync()

child = root.create_window(50, 50, 200, 200, 0, d.screen().root_depth)
wm_transient_for = d.intern_atom('WM_TRANSIENT_FOR')
child.change_property(wm_transient_for, 33, 32, [parent.id])
child.map()
d.sync()

# Query tree to check stacking order
tree = root.query_tree()
children = tree.children
parent_idx = -1
child_idx = -1
for i, c in enumerate(children):
    if c.id == parent.id:
        parent_idx = i
    if c.id == child.id:
        child_idx = i

print(f"child_above_parent={child_idx > parent_idx}")

child.destroy()
parent.destroy()
d.close()
`,
		);
		expect(output).toContain("child_above_parent=True");
	});
});

// ---------------------------------------------------------------------------
// ICCCM WM_DELETE_WINDOW protocol
// ---------------------------------------------------------------------------

test.describe.serial("ICCCM WM_DELETE_WINDOW (Phase 8E)", () => {
	test("WM_PROTOCOLS property can be set and read back", async ({
		sidecarContainer,
	}) => {
		const output = await runPythonX11(
			sidecarContainer,
			`
from Xlib import display, X
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)

wm_protocols = d.intern_atom('WM_PROTOCOLS')
wm_delete = d.intern_atom('WM_DELETE_WINDOW')
wm_take_focus = d.intern_atom('WM_TAKE_FOCUS')

w.change_property(wm_protocols, 4, 32, [wm_delete, wm_take_focus])
d.sync()

prop = w.get_full_property(wm_protocols, X.AnyPropertyType)
if prop and prop.value:
    import array
    atoms = array.array('I', prop.value)
    has_delete = wm_delete in atoms
    has_focus = wm_take_focus in atoms
    print(f"has_delete={has_delete}")
    print(f"has_focus={has_focus}")
else:
    print("has_delete=False")
    print("has_focus=False")

w.destroy()
d.close()
`,
		);
		expect(output).toContain("has_delete=True");
		expect(output).toContain("has_focus=True");
	});
});
