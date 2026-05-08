import Xlib.display
import Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create test window for passive grab
w = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    window_class=Xlib.X.InputOutput,
    visual=Xlib.X.CopyFromParent)
w.map()
d.sync()

# Passive button grab: grab button 1 on this window
w.grab_button(
    1,  # button
    Xlib.X.AnyModifier,
    True,  # owner_events
    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync,
    Xlib.X.GrabModeAsync,
    0,  # confine_to
    0   # cursor
)
d.sync()
print("passive_grab=ok")

# Ungrab
w.ungrab_button(1, Xlib.X.AnyModifier)
d.sync()
print("passive_ungrab=ok")

w.destroy()
d.sync()
d.close()
