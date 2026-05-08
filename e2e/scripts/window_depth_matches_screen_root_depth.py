from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
geo = root.get_geometry()
print(f"root_depth={geo.depth}")
# Create window and check its depth
w = root.create_window(
    0, 0, 100, 100, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
)
geo2 = w.get_geometry()
print(f"window_depth={geo2.depth}")
w.destroy()
d.close()
