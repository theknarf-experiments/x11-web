import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

w = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# GrabKeyboard: in python-xlib the method lives on Window, not Display.
status = w.grab_keyboard(True,
    Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync,
    Xlib.X.CurrentTime)
if status == 0:  # GrabSuccess
    print("GRAB_KEYBOARD_OK")
else:
    print(f"GRAB_KEYBOARD_STATUS:{status}")

# UngrabKeyboard is on Display.
d.ungrab_keyboard(Xlib.X.CurrentTime)
d.sync()
print("UNGRAB_KEYBOARD_OK")

w.destroy()
d.sync()
