import Xlib.display
import Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create test window
w = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    window_class=Xlib.X.InputOutput,
    visual=Xlib.X.CopyFromParent)
w.map()
d.sync()

# Passive key grab: grab keycode 38 (usually 'a')
w.grab_key(
    38,  # keycode
    Xlib.X.AnyModifier,
    True,  # owner_events
    Xlib.X.GrabModeAsync,
    Xlib.X.GrabModeAsync
)
d.sync()
print("key_grab=ok")

# Ungrab
w.ungrab_key(38, Xlib.X.AnyModifier)
d.sync()
print("key_ungrab=ok")

w.destroy()
d.sync()
d.close()
