import Xlib.display, Xlib.X, Xlib.Xatom, sys
import time, threading, struct
passed = 0; failed = 0

d_owner = Xlib.display.Display()
root = d_owner.screen().root
CLIPBOARD = d_owner.intern_atom("CLIPBOARD")
TARGETS = d_owner.intern_atom("TARGETS")
UTF8 = d_owner.intern_atom("UTF8_STRING")
TEXT = d_owner.intern_atom("TEXT")
SEL_PROP = d_owner.intern_atom("XTEST_TARGETS_PROP")

w_owner = root.create_window(0, 0, 1, 1, 0, d_owner.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w_owner.set_selection_owner(CLIPBOARD, Xlib.X.CurrentTime)
d_owner.sync()

# Handle SelectionRequest: respond to TARGETS with a list of supported types,
# and respond to UTF8_STRING with actual data
request_done = threading.Event()
supported_targets = [TARGETS, UTF8, TEXT, Xlib.Xatom.STRING]

def handle_requests():
    for _ in range(200):
        while d_owner.pending_events():
            ev = d_owner.next_event()
            if ev.type == Xlib.X.SelectionRequest:
                if ev.target == TARGETS:
                    # Return list of supported targets as ATOM array
                    ev.requestor.change_property(ev.property,
                        Xlib.Xatom.ATOM, 32, supported_targets)
                elif ev.target == UTF8:
                    ev.requestor.change_property(ev.property, UTF8, 8,
                        b"targets-test-data")
                else:
                    ev.requestor.change_property(ev.property,
                        ev.target, 8, b"fallback")
                d_owner.sync()
                resp = Xlib.protocol.event.SelectionNotify(
                    time=ev.time,
                    requestor=ev.requestor,
                    selection=ev.selection,
                    target=ev.target,
                    property=ev.property)
                ev.requestor.send_event(resp)
                d_owner.sync()
                request_done.set()
        time.sleep(0.03)

t = threading.Thread(target=handle_requests, daemon=True)
t.start()

# Requestor asks for TARGETS
d_req = Xlib.display.Display()
w_req = d_req.screen().root.create_window(0, 0, 1, 1, 0,
    d_req.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)

w_req.convert_selection(CLIPBOARD, TARGETS, SEL_PROP, Xlib.X.CurrentTime)
d_req.sync()
request_done.wait(timeout=5.0)
time.sleep(0.3)
d_req.sync()

# Drain events
for _ in range(50):
    while d_req.pending_events():
        ev = d_req.next_event()
    time.sleep(0.02)

# Test 1: TARGETS property contains atom list
try:
    prop = w_req.get_property(SEL_PROP, Xlib.Xatom.ATOM, 0, 1000)
    if prop and len(prop.value) >= 3:
        passed += 1; print(f"PASS: TARGETS returned {len(prop.value)} target types")
        target_list = list(prop.value)
        # Test 2: TARGETS includes UTF8_STRING
        if UTF8 in target_list:
            passed += 1; print("PASS: TARGETS includes UTF8_STRING")
        else:
            failed += 1; print(f"FAIL: UTF8_STRING not in targets {target_list}")
        # Test 3: TARGETS includes STRING
        if Xlib.Xatom.STRING in target_list:
            passed += 1; print("PASS: TARGETS includes STRING")
        else:
            failed += 1; print(f"FAIL: STRING not in targets {target_list}")
        # Test 4: TARGETS includes TARGETS itself
        if TARGETS in target_list:
            passed += 1; print("PASS: TARGETS includes TARGETS")
        else:
            failed += 1; print(f"FAIL: TARGETS not in targets {target_list}")
    else:
        failed += 1; print(f"FAIL: TARGETS returned empty or too few")
except Exception as e:
    failed += 1; print(f"FAIL: TARGETS property: {e}")

# Test 5: Request UTF8_STRING target and verify data
try:
    request_done.clear()
    SEL_PROP2 = d_req.intern_atom("XTEST_TARGETS_PROP2")
    w_req.convert_selection(CLIPBOARD, UTF8, SEL_PROP2, Xlib.X.CurrentTime)
    d_req.sync()
    request_done.wait(timeout=5.0)
    time.sleep(0.3)
    d_req.sync()
    for _ in range(50):
        while d_req.pending_events():
            d_req.next_event()
        time.sleep(0.02)
    prop2 = w_req.get_property(SEL_PROP2, UTF8, 0, 10000)
    if prop2 and prop2.value == b"targets-test-data":
        passed += 1; print("PASS: UTF8_STRING target returns correct data")
    else:
        val = prop2.value if prop2 else None
        failed += 1; print(f"FAIL: UTF8_STRING data = {val}")
except Exception as e:
    failed += 1; print(f"FAIL: UTF8_STRING conversion: {e}")

w_owner.destroy()
w_req.destroy()
d_owner.close()
d_req.close()
print(f"xts-selection-targets: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
