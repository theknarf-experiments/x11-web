import Xlib.display, Xlib.X

d = Xlib.display.Display()
s = d.screen()
root = s.root

# Create a window
w = root.create_window(
    0, 0, 100, 100, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
)
w.map()
d.sync()

# Set close-down mode to RetainTemporary
d.set_close_down_mode(Xlib.X.RetainTemporary)
d.sync()

print('killclient-ok: set RetainTemporary mode')

w.destroy()
d.close()
