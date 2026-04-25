import Xlib.display, Xlib.X, Xlib.Xatom, sys
import time, threading
passed = 0; failed = 0

PAYLOAD = b"Hello from X11 clipboard test 12345"

# Owner connection
d_owner = Xlib.display.Display()
root = d_owner.screen().root
CLIPBOARD = d_owner.intern_atom("CLIPBOARD")
UTF8 = d_owner.intern_atom("UTF8_STRING")
SEL_PROP = d_owner.intern_atom("XTEST_CLIP_PROP")

w_owner = root.create_window(0, 0, 1, 1, 0, d_owner.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w_owner.set_selection_owner(CLIPBOARD, Xlib.X.CurrentTime)
d_owner.sync()

# Verify ownership
owner = d_owner.get_selection_owner(CLIPBOARD)
if owner.id == w_owner.id:
    passed += 1; print("PASS: clipboard owner set")
else:
    failed += 1; print(f"FAIL: owner mismatch")

# Start a thread to handle SelectionRequest from owner side
request_handled = threading.Event()
def handle_requests():
    for _ in range(100):
        while d_owner.pending_events():
            ev = d_owner.next_event()
            if ev.type == Xlib.X.SelectionRequest:
                ev.requestor.change_property(ev.property, UTF8, 8, PAYLOAD)
                d_owner.sync()
                resp = Xlib.protocol.event.SelectionNotify(
                    time=ev.time,
                    requestor=ev.requestor,
                    selection=ev.selection,
                    target=ev.target,
                    property=ev.property)
                ev.requestor.send_event(resp)
                d_owner.sync()
                request_handled.set()
                return
        time.sleep(0.05)

t = threading.Thread(target=handle_requests, daemon=True)
t.start()

# Requestor connection (separate client)
d_req = Xlib.display.Display()
w_req = d_req.screen().root.create_window(0, 0, 1, 1, 0,
    d_req.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)

# Request the clipboard selection
w_req.convert_selection(CLIPBOARD, UTF8, SEL_PROP, Xlib.X.CurrentTime)
d_req.sync()

# Wait for the owner thread to handle the request
request_handled.wait(timeout=5.0)
time.sleep(0.3)
d_req.sync()

# Read SelectionNotify and the property
got_notify = False
for _ in range(50):
    while d_req.pending_events():
        ev = d_req.next_event()
        if ev.type == Xlib.X.SelectionNotify:
            got_notify = True
    if got_notify:
        break
    time.sleep(0.05)

if got_notify:
    passed += 1; print("PASS: SelectionNotify received by requestor")
else:
    failed += 1; print("FAIL: no SelectionNotify received")

# Verify clipboard content
try:
    prop = w_req.get_property(SEL_PROP, UTF8, 0, 10000)
    if prop and prop.value == PAYLOAD:
        passed += 1; print(f"PASS: clipboard content matches ({len(PAYLOAD)} bytes)")
    else:
        val = prop.value if prop else None
        failed += 1; print(f"FAIL: clipboard content = {val}")
except Exception as e:
    failed += 1; print(f"FAIL: GetProperty: {e}")

w_owner.destroy()
w_req.destroy()
d_owner.close()
d_req.close()
print(f"xts-clipboard-roundtrip: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
