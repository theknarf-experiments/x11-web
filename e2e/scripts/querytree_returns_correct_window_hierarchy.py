import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)
child1 = parent.create_window(10, 10, 50, 50, 0, screen.root_depth)
child2 = parent.create_window(70, 10, 50, 50, 0, screen.root_depth)
d.sync()

tree = parent.query_tree()
child_ids = [c.id for c in tree.children]
print(f"n_children={len(child_ids)}")
print(f"has_child1={child1.id in child_ids}")
print(f"has_child2={child2.id in child_ids}")
print(f"parent_is_root={tree.parent.id == screen.root.id}")

child1.destroy()
child2.destroy()
parent.destroy()
d.close()
