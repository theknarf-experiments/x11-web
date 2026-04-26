import Xlib.display, Xlib.X, Xlib.Xatom, sys
from Xlib.protocol import request as xrequest

passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
CLIPBOARD = d.intern_atom("CLIPBOARD")
PRIMARY = d.intern_atom("PRIMARY")

def clear_owner(selection):
    # python-xlib only exposes Window.set_selection_owner (always self),
    # so to clear we send the raw SetSelectionOwner with window=NONE.
    xrequest.SetSelectionOwner(
        display=d.display,
        window=Xlib.X.NONE,
        selection=selection,
        time=Xlib.X.CurrentTime,
    )

# Reset selection state first — earlier tests in the run may have
# left ownership behind on the shared sidecar.
clear_owner(CLIPBOARD)
clear_owner(PRIMARY)
d.sync()

# Test 1: No selection owner after explicit clear
owner = d.get_selection_owner(CLIPBOARD)
if owner == Xlib.X.NONE:
    passed += 1
else:
    print(f"FAIL: CLIPBOARD owner should be None, got {owner}")
    failed += 1
# Test 2: Set and get selection owner
w = root.create_window(0, 0, 1, 1, 0, 24, Xlib.X.InputOutput)
w.set_selection_owner(CLIPBOARD, Xlib.X.CurrentTime)
d.sync()
owner2 = d.get_selection_owner(CLIPBOARD)
if owner2 == w:
    passed += 1
else:
    print(f"FAIL: CLIPBOARD owner should be {w}, got {owner2}")
    failed += 1
# Test 3: Clear selection ownership
clear_owner(CLIPBOARD)
d.sync()
owner3 = d.get_selection_owner(CLIPBOARD)
if owner3 == Xlib.X.NONE:
    passed += 1
else:
    print(f"FAIL: CLIPBOARD should be cleared, got {owner3}")
    failed += 1
# Test 4: PRIMARY selection works similarly
w.set_selection_owner(PRIMARY, Xlib.X.CurrentTime)
d.sync()
owner4 = d.get_selection_owner(PRIMARY)
if owner4 == w:
    passed += 1
else:
    print(f"FAIL: PRIMARY owner should be {w}, got {owner4}")
    failed += 1
w.destroy()
d.close()
print(f"selection-protocol: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
