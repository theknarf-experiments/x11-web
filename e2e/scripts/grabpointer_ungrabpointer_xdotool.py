import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    # GrabPointer on root window
    status = root.grab_pointer(
        True,  # owner_events
        Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
        Xlib.X.GrabModeAsync,
        Xlib.X.GrabModeAsync,
        Xlib.X.NONE,  # confine_to
        Xlib.X.NONE,  # cursor
        Xlib.X.CurrentTime
    )
    d.sync()
    # python-xlib returns the integer status code directly, not a struct.
    status_code = status.status if hasattr(status, "status") else status
    if status_code == 0:
        passed += 1; print("PASS: GrabPointer succeeded")
    else:
        failed += 1; print(f"FAIL: GrabPointer status={status_code}")
    # UngrabPointer
    d.ungrab_pointer(Xlib.X.CurrentTime)
    d.sync()
    passed += 1; print("PASS: UngrabPointer completed")
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
try:
    # GrabKeyboard
    status = root.grab_keyboard(
        True,
        Xlib.X.GrabModeAsync,
        Xlib.X.GrabModeAsync,
        Xlib.X.CurrentTime
    )
    d.sync()
    status_code = status.status if hasattr(status, "status") else status
    if status_code == 0:
        passed += 1; print("PASS: GrabKeyboard succeeded")
    else:
        failed += 1; print(f"FAIL: GrabKeyboard status={status_code}")
    d.ungrab_keyboard(Xlib.X.CurrentTime)
    d.sync()
    passed += 1; print("PASS: UngrabKeyboard completed")
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"grabs-basic: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
