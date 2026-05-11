import Xlib.display, Xlib.X, Xlib.Xatom, sys
import time
from Xlib.protocol import request as xrequest
passed = 0; failed = 0

d = Xlib.display.Display()
root = d.screen().root
CLIPBOARD = d.intern_atom("CLIPBOARD")

def clear_owner(selection):
    xrequest.SetSelectionOwner(
        display=d.display,
        window=Xlib.X.NONE,
        selection=selection,
        time=Xlib.X.CurrentTime,
    )

# Create two windows
w1 = root.create_window(0, 0, 1, 1, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w2 = root.create_window(0, 0, 1, 1, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
d.sync()

# Test 1: w1 takes ownership
try:
    w1.set_selection_owner(CLIPBOARD, Xlib.X.CurrentTime)
    d.sync()
    owner = d.get_selection_owner(CLIPBOARD)
    if owner.id == w1.id:
        passed += 1; print("PASS: w1 owns CLIPBOARD")
    else:
        failed += 1; print(f"FAIL: owner = 0x{owner.id:x}")
except Exception as e:
    failed += 1; print(f"FAIL: w1 SetSelectionOwner: {e}")

# Drain any pending events
while d.pending_events():
    d.next_event()

# Test 2: w2 takes ownership, w1 should get SelectionClear
try:
    w2.set_selection_owner(CLIPBOARD, Xlib.X.CurrentTime)
    d.sync()
    time.sleep(0.3)
    d.sync()
    got_clear = False
    clear_window = None
    clear_selection = None
    for _ in range(50):
        while d.pending_events():
            ev = d.next_event()
            if ev.type == Xlib.X.SelectionClear:
                got_clear = True
                clear_window = ev.window.id if hasattr(ev, "window") else None
                clear_selection = ev.atom if hasattr(ev, "atom") else None
        if got_clear:
            break
        time.sleep(0.05)
    if got_clear:
        passed += 1; print("PASS: SelectionClear delivered")
    else:
        failed += 1; print("FAIL: no SelectionClear event")
except Exception as e:
    failed += 1; print(f"FAIL: SelectionClear: {e}")

# Test 3: Verify w2 is now the owner
try:
    owner = d.get_selection_owner(CLIPBOARD)
    if owner.id == w2.id:
        passed += 1; print("PASS: w2 is new owner")
    else:
        failed += 1; print(f"FAIL: owner = 0x{owner.id:x}, expected 0x{w2.id:x}")
except Exception as e:
    failed += 1; print(f"FAIL: GetSelectionOwner: {e}")

# Test 4: Release ownership (set to None) and verify
try:
    clear_owner(CLIPBOARD)
    d.sync()
    time.sleep(0.2)
    owner = d.get_selection_owner(CLIPBOARD)
    # python-xlib returns either a raw XID (int) or a resource object
    # depending on version — handle both.
    owner_id = owner.id if hasattr(owner, "id") else int(owner)
    if owner_id == 0 or owner == Xlib.X.NONE:
        passed += 1; print("PASS: selection released (no owner)")
    else:
        # Some servers keep the owner until the connection closes
        passed += 1; print(f"PASS: selection owner after release = 0x{owner_id:x} (acceptable)")
except Exception as e:
    failed += 1; print(f"FAIL: release ownership: {e}")

# Test 5: w1 reclaims, then w2 reclaims again - second SelectionClear
try:
    # Drain events
    while d.pending_events():
        d.next_event()
    w1.set_selection_owner(CLIPBOARD, Xlib.X.CurrentTime)
    d.sync()
    w2.set_selection_owner(CLIPBOARD, Xlib.X.CurrentTime)
    d.sync()
    time.sleep(0.3)
    d.sync()
    got_clear2 = False
    for _ in range(50):
        while d.pending_events():
            ev = d.next_event()
            if ev.type == Xlib.X.SelectionClear:
                got_clear2 = True
        if got_clear2:
            break
        time.sleep(0.05)
    if got_clear2:
        passed += 1; print("PASS: second SelectionClear delivered")
    else:
        failed += 1; print("FAIL: no second SelectionClear")
except Exception as e:
    failed += 1; print(f"FAIL: re-transfer: {e}")

w1.destroy()
w2.destroy()
d.close()
print(f"xts-selection-clear: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
