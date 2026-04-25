import Xlib.display, Xlib.X
d = Xlib.display.Display()
s = d.screen()
root = s.root
# Create window with center gravity
w = root.create_window(
    10, 10, 100, 100, 0,
    s.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    bit_gravity=Xlib.X.CenterGravity,
    event_mask=Xlib.X.ExposureMask,
)
w.map()
d.sync()
# Resize
w.configure(width=200, height=200)
d.sync()
g = w.get_geometry()
print(f'gravity-resize: w={g.width} h={g.height}')
w.destroy()
d.close()
