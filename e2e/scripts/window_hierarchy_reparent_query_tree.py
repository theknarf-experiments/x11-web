import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create parent and child windows
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
child = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
parent.map()
child.map()
d.sync()

# Reparent child into parent
child.reparent(parent, 10, 10)
d.sync()

# Query tree to verify
tree = parent.query_tree()
child_ids = [c.id for c in tree.children]
if child.id in child_ids:
    print("REPARENT_OK")
else:
    print(f"FAIL: child {child.id} not in {child_ids}")

# Verify geometry relative to new parent
geom = child.get_geometry()
print(f"child_x={geom.x} child_y={geom.y}")
if geom.x == 10 and geom.y == 10:
    print("GEOMETRY_OK")

d.close()
