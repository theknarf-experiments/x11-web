import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Create a window and set input focus
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask | Xlib.X.KeyPressMask)
w.map()
d.sync()

# Set focus to window
d.set_input_focus(w, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

# Verify focus
focus = d.get_input_focus()
if focus.focus == w:
    print("FOCUS_SET_OK")
else:
    print(f"FOCUS_SET_FAIL: expected {w}, got {focus.focus}")

# Set focus to PointerRoot
d.set_input_focus(Xlib.X.PointerRoot, Xlib.X.RevertToPointerRoot, Xlib.X.CurrentTime)
d.sync()

focus2 = d.get_input_focus()
if focus2.focus == Xlib.X.PointerRoot:
    print("FOCUS_POINTERROOT_OK")
else:
    print(f"FOCUS_POINTERROOT_FAIL: got {focus2.focus}")
