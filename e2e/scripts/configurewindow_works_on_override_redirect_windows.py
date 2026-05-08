import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create an OR window and configure it
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    override_redirect=True)
w.map()
d.sync()

# Move and resize
w.configure(x=50, y=50, width=200, height=150)
d.sync()

# Verify
geom = w.get_geometry()
if geom.x == 50 and geom.y == 50 and geom.width == 200 and geom.height == 150:
    print("OR_CONFIGURE_OK")
else:
    print(f"OR_CONFIGURE: x={geom.x} y={geom.y} w={geom.width} h={geom.height}")

d.close()
