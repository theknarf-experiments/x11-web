import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(10, 10, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.KeyPressMask)
w.map()
d.sync()

# Grab key 'a' (keycode 38)
w.grab_key(38, Xlib.X.AnyModifier, True,
    Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync)
d.sync()
print("key_grab_established=True")

# Ungrab
w.ungrab_key(38, Xlib.X.AnyModifier)
d.sync()
print("key_grab_removed=True")

w.destroy()
d.close()
