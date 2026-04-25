import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    w = root.create_window(0, 0, 200, 200, 0, 24, Xlib.X.InputOutput)
    w.map()
    d.sync()
    # GrabButton: passive grab on button 1
    w.grab_button(
        1,  # button
        Xlib.X.AnyModifier,
        True,  # owner_events
        Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
        Xlib.X.GrabModeAsync,
        Xlib.X.GrabModeAsync,
        Xlib.X.NONE,
        Xlib.X.NONE
    )
    d.sync()
    passed += 1; print("PASS: GrabButton succeeded")
    # UngrabButton
    w.ungrab_button(1, Xlib.X.AnyModifier)
    d.sync()
    passed += 1; print("PASS: UngrabButton succeeded")
    # GrabKey: passive grab on key
    w.grab_key(10, Xlib.X.AnyModifier, True,
        Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync)
    d.sync()
    passed += 1; print("PASS: GrabKey succeeded")
    w.ungrab_key(10, Xlib.X.AnyModifier)
    d.sync()
    passed += 1; print("PASS: UngrabKey succeeded")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"grabs-passive: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
