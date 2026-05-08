from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Create and map a window
w = root.create_window(
    10, 10, 100, 100, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
    event_mask=X.ExposureMask,
)
w.map()
d.sync()
# Redirect the window
composite_ext = d.query_extension('Composite')
if composite_ext and composite_ext.present:
    print("composite_available=True")
else:
    print("composite_available=False")
w.destroy()
d.close()
