import Xlib.display, Xlib.X, Xlib.Xatom, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
CLIPBOARD = d.intern_atom("CLIPBOARD")
PRIMARY = d.intern_atom("PRIMARY")
# Test 1: No selection owner initially
owner = d.get_selection_owner(CLIPBOARD)
if owner == Xlib.X.NONE:
    passed += 1
else:
    print(f"FAIL: CLIPBOARD owner should be None, got {owner}")
    failed += 1
# Test 2: Set and get selection owner
w = root.create_window(0, 0, 1, 1, 0, 24, Xlib.X.InputOutput)
d.set_selection_owner(CLIPBOARD, w, Xlib.X.CurrentTime)
d.sync()
owner2 = d.get_selection_owner(CLIPBOARD)
if owner2 == w:
    passed += 1
else:
    print(f"FAIL: CLIPBOARD owner should be {w}, got {owner2}")
    failed += 1
# Test 3: Clear selection ownership
d.set_selection_owner(CLIPBOARD, Xlib.X.NONE, Xlib.X.CurrentTime)
d.sync()
owner3 = d.get_selection_owner(CLIPBOARD)
if owner3 == Xlib.X.NONE:
    passed += 1
else:
    print(f"FAIL: CLIPBOARD should be cleared, got {owner3}")
    failed += 1
# Test 4: PRIMARY selection works similarly
d.set_selection_owner(PRIMARY, w, Xlib.X.CurrentTime)
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
