import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(10, 10, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask)
w.map()
d.sync()

# Establish a passive button grab on button 1
w.grab_button(1, Xlib.X.AnyModifier,
    True,
    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync,
    Xlib.X.NONE, Xlib.X.NONE)
d.sync()
print("passive_grab_established=True")

# Ungrab
w.ungrab_button(1, Xlib.X.AnyModifier)
d.sync()
print("passive_grab_removed=True")

w.destroy()
d.close()
