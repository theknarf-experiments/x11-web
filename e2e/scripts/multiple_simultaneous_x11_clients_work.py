import Xlib.display, Xlib.X

# Open two separate connections
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()

screen1 = d1.screen()
screen2 = d2.screen()

# Create windows on each connection
w1 = screen1.root.create_window(0, 0, 50, 50, 0, screen1.root_depth)
w2 = screen2.root.create_window(60, 0, 50, 50, 0, screen2.root_depth)

w1.map()
w2.map()
d1.sync()
d2.sync()

# Both windows should be queryable from either connection
tree1 = screen1.root.query_tree()
child_ids = [c.id for c in tree1.children]
# At least our two windows should be there (plus possibly root children)
print(f"tree_has_w1={w1.id in child_ids}")
print(f"tree_has_w2={w2.id in child_ids}")
print(f"total_children={len(child_ids)}")

w1.destroy()
w2.destroy()
d1.close()
d2.close()
