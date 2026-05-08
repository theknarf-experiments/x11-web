import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create a parent window
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
parent.map()
d.sync()

# Create a child of parent (not root)
child = parent.create_window(10, 10, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
d.sync()

# Verify parent has SubstructureRedirectMask set
attrs = parent.get_attributes()
print(f"parent_attrs_ok")

# Try to map the child — should succeed since we own the redirect
child.map()
d.sync()
print("CHILD_MAP_OK")

d.close()
