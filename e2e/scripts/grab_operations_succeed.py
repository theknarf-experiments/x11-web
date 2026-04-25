import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, 24, Xlib.X.InputOutput)
w.map()
d.sync()
# Test 1: GrabPointer
status = w.grab_pointer(True, Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync, Xlib.X.NONE, Xlib.X.NONE, Xlib.X.CurrentTime)
if status == Xlib.X.GrabSuccess:
    passed += 1
else:
    print(f"FAIL: GrabPointer status={status}")
    failed += 1
# Test 2: UngrabPointer
d.ungrab_pointer(Xlib.X.CurrentTime)
d.sync()
passed += 1
# Test 3: GrabKeyboard
status2 = w.grab_keyboard(True, Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync, Xlib.X.CurrentTime)
if status2 == Xlib.X.GrabSuccess:
    passed += 1
else:
    print(f"FAIL: GrabKeyboard status={status2}")
    failed += 1
# Test 4: UngrabKeyboard
d.ungrab_keyboard(Xlib.X.CurrentTime)
d.sync()
passed += 1
# Test 5: GrabServer / UngrabServer
d.grab_server()
d.sync()
d.ungrab_server()
d.sync()
passed += 1
w.destroy()
d.close()
print(f"grabs: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
