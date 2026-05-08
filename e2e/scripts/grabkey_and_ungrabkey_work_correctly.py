import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

w = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# GrabKey: passive grab on keycode 38 (a) with no modifier
w.grab_key(38, 0, True, Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync)
d.sync()
print("GRAB_KEY_OK")

# UngrabKey
w.ungrab_key(38, 0)
d.sync()
print("UNGRAB_KEY_OK")

w.destroy()
d.sync()
