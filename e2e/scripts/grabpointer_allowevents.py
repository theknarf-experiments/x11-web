from Xlib import X, display
d = display.Display(":99")
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)
w.map()
d.sync()
# Grab pointer in synchronous mode
status = w.grab_pointer(True, X.ButtonPressMask, X.GrabModeSync, X.GrabModeAsync,
    X.NONE, X.NONE, X.CurrentTime)
assert status == X.GrabSuccess, f"GrabPointer failed: {status}"
d.sync()
# Ungrab
d.ungrab_pointer(X.CurrentTime)
d.sync()
# Grab keyboard in async mode
status = w.grab_keyboard(True, X.GrabModeAsync, X.GrabModeAsync, X.CurrentTime)
assert status == X.GrabSuccess, f"GrabKeyboard failed: {status}"
d.ungrab_keyboard(X.CurrentTime)
d.sync()
w.destroy()
d.close()
print("grab-test-pass")
