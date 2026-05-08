from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Create parent
parent = root.create_window(0, 0, 200, 200, 0, screen.root_depth)
# Create children
child1 = parent.create_window(10, 10, 50, 50, 0, screen.root_depth)
child2 = parent.create_window(70, 10, 50, 50, 0, screen.root_depth)
d.sync()
# QueryTree
tree = parent.query_tree()
print(f"parent_of_parent={tree.parent.id == root.id}")
print(f"num_children={len(tree.children)}")
child_ids = [c.id for c in tree.children]
print(f"child1_in_tree={child1.id in child_ids}")
print(f"child2_in_tree={child2.id in child_ids}")
child1.destroy()
child2.destroy()
parent.destroy()
d.close()
