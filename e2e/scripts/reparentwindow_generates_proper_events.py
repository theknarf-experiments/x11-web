import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create parent windows
parent1 = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    event_mask=Xlib.X.SubstructureNotifyMask)
parent2 = screen.root.create_window(200, 0, 200, 200, 0, screen.root_depth,
    event_mask=Xlib.X.SubstructureNotifyMask)
child = parent1.create_window(10, 10, 50, 50, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)
d.sync()

# Verify child is under parent1
tree1 = parent1.query_tree()
print(f"before_n_children_p1={len(tree1.children)}")

# Reparent child to parent2 at (20, 20)
child.reparent(parent2, 20, 20)
d.sync()

# Verify child moved to parent2
tree1_after = parent1.query_tree()
tree2_after = parent2.query_tree()
print(f"after_n_children_p1={len(tree1_after.children)}")
print(f"after_n_children_p2={len(tree2_after.children)}")

# Verify geometry was updated
geo = child.get_geometry()
print(f"child_x={geo.x} child_y={geo.y}")

child.destroy()
parent1.destroy()
parent2.destroy()
d.close()
