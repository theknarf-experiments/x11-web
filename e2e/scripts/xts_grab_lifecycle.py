import Xlib.display, Xlib.X, sys, time
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

w = root.create_window(0, 0, 200, 200, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ButtonPressMask | Xlib.X.KeyPressMask)
w.map()
d.sync()
time.sleep(0.2)

# Test 1: GrabPointer
status = w.grab_pointer(
    True,
    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync,
    Xlib.X.GrabModeAsync,
    Xlib.X.NONE,
    Xlib.X.NONE,
    Xlib.X.CurrentTime)
if status == Xlib.X.GrabSuccess:
    passed += 1; print("PASS: GrabPointer succeeded")
else:
    failed += 1; print(f"FAIL: GrabPointer returned {status}")

# Test 2: UngrabPointer
d.ungrab_pointer(Xlib.X.CurrentTime)
d.sync()
passed += 1; print("PASS: UngrabPointer completed")

# Test 3: GrabKeyboard
status = w.grab_keyboard(
    True,
    Xlib.X.GrabModeAsync,
    Xlib.X.GrabModeAsync,
    Xlib.X.CurrentTime)
if status == Xlib.X.GrabSuccess:
    passed += 1; print("PASS: GrabKeyboard succeeded")
else:
    failed += 1; print(f"FAIL: GrabKeyboard returned {status}")

# Test 4: UngrabKeyboard
d.ungrab_keyboard(Xlib.X.CurrentTime)
d.sync()
passed += 1; print("PASS: UngrabKeyboard completed")

# Test 5: GrabButton (passive grab)
try:
    w.grab_button(
        Xlib.X.AnyButton,
        Xlib.X.AnyModifier,
        True,
        Xlib.X.ButtonPressMask,
        Xlib.X.GrabModeAsync,
        Xlib.X.GrabModeAsync,
        Xlib.X.NONE,
        Xlib.X.NONE)
    d.sync()
    passed += 1; print("PASS: GrabButton passive grab set")
except Exception as e:
    failed += 1; print(f"FAIL: GrabButton: {e}")

# Test 6: UngrabButton
try:
    w.ungrab_button(Xlib.X.AnyButton, Xlib.X.AnyModifier)
    d.sync()
    passed += 1; print("PASS: UngrabButton completed")
except Exception as e:
    failed += 1; print(f"FAIL: UngrabButton: {e}")

# Test 7: GrabKey (passive grab)
try:
    w.grab_key(Xlib.X.AnyKey, Xlib.X.AnyModifier,
        True, Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync)
    d.sync()
    passed += 1; print("PASS: GrabKey passive grab set")
except Exception as e:
    failed += 1; print(f"FAIL: GrabKey: {e}")

# Test 8: UngrabKey
try:
    w.ungrab_key(Xlib.X.AnyKey, Xlib.X.AnyModifier)
    d.sync()
    passed += 1; print("PASS: UngrabKey completed")
except Exception as e:
    failed += 1; print(f"FAIL: UngrabKey: {e}")

w.destroy()
d.close()
print(f"xts-grabs: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
