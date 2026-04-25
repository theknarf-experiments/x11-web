import Xlib.display, Xlib.X, Xlib.Xatom, sys
import time
passed = 0; failed = 0

d = Xlib.display.Display()
root = d.screen().root
CLIPBOARD = d.intern_atom("CLIPBOARD")
UTF8 = d.intern_atom("UTF8_STRING")
MY_PROP = d.intern_atom("XTEST_SEL_PROP")

# Create two windows
w1 = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w2 = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
d.sync()

# Test 1: SetSelectionOwner on w1
try:
    w1.set_selection_owner(CLIPBOARD, Xlib.X.CurrentTime)
    d.sync()
    owner = d.get_selection_owner(CLIPBOARD)
    if owner.id == w1.id:
        passed += 1; print("PASS: w1 is selection owner")
    else:
        failed += 1; print(f"FAIL: owner is 0x{owner.id:x}, expected 0x{w1.id:x}")
except Exception as e:
    failed += 1; print(f"FAIL: SetSelectionOwner: {e}")

# Test 2: ConvertSelection from w2 triggers SelectionRequest on w1
# and SelectionNotify on w2
try:
    w2.convert_selection(CLIPBOARD, UTF8, MY_PROP, Xlib.X.CurrentTime)
    d.sync()
    time.sleep(0.3)
    d.sync()
    # Check for SelectionRequest on the owner side
    got_request = False
    got_notify = False
    for _ in range(50):
        while d.pending_events():
            ev = d.next_event()
            if ev.type == Xlib.X.SelectionRequest:
                got_request = True
                # Respond with the selection data
                resp = Xlib.protocol.event.SelectionNotify(
                    time=ev.time,
                    requestor=ev.requestor,
                    selection=ev.selection,
                    target=ev.target,
                    property=ev.property)
                # Set property on requestor
                ev.requestor.change_property(ev.property, UTF8, 8,
                    b"selection-transfer-data")
                d.sync()
                ev.requestor.send_event(resp)
                d.sync()
            elif ev.type == Xlib.X.SelectionNotify:
                got_notify = True
        if got_request and got_notify:
            break
        time.sleep(0.05)
    if got_request:
        passed += 1; print("PASS: SelectionRequest delivered to owner")
    else:
        failed += 1; print("FAIL: no SelectionRequest received")
    if got_notify:
        passed += 1; print("PASS: SelectionNotify delivered to requestor")
    else:
        failed += 1; print("FAIL: no SelectionNotify received")
except Exception as e:
    failed += 1; print(f"FAIL: selection transfer: {e}")

# Test 3: Verify the property was set on w2
try:
    prop = w2.get_property(MY_PROP, UTF8, 0, 1000)
    if prop and prop.value == b"selection-transfer-data":
        passed += 1; print("PASS: selection data transferred correctly")
    else:
        val = prop.value if prop else None
        failed += 1; print(f"FAIL: transferred data = {val}")
except Exception as e:
    failed += 1; print(f"FAIL: GetProperty on transfer: {e}")

w1.destroy()
w2.destroy()
d.close()
print(f"xts-selection-transfer: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
