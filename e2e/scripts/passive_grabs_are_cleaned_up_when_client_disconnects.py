import Xlib.display, Xlib.X
import os, time

# First client: create window and grab
d1 = Xlib.display.Display()
screen = d1.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ButtonPressMask)
w.map()
d1.sync()

# Set a passive button grab on this window
w.grab_button(1, Xlib.X.AnyModifier, True,
    Xlib.X.ButtonPressMask, Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync,
    Xlib.X.NONE, Xlib.X.NONE)
d1.sync()

# Disconnect the first client (window gets destroyed)
d1.close()

# Second client: connect and verify no stale state causes issues
d2 = Xlib.display.Display()
screen2 = d2.screen()
w2 = screen2.root.create_window(0, 0, 100, 100, 0, screen2.root_depth,
    event_mask=Xlib.X.ExposureMask)
w2.map()
d2.sync()
print("cleanup_ok=True")
w2.destroy()
d2.close()
