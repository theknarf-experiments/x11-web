import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

w = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# GrabButton: passive grab on button 1 with any modifier
w.grab_button(1, Xlib.X.AnyModifier, True,
    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync,
    Xlib.X.NONE, Xlib.X.NONE)
d.sync()
print("GRAB_BUTTON_OK")

# UngrabButton: release the grab
w.ungrab_button(1, Xlib.X.AnyModifier)
d.sync()
print("UNGRAB_BUTTON_OK")

w.destroy()
d.sync()
