from Xlib import X, display
d = display.Display()
root = d.screen().root

parent = root.create_window(0, 0, 400, 400, 0, d.screen().root_depth)
parent.map()
d.sync()

# Create 3 children
c1 = parent.create_window(10, 10, 50, 50, 0, d.screen().root_depth)
c2 = parent.create_window(20, 20, 50, 50, 0, d.screen().root_depth)
c3 = parent.create_window(30, 30, 50, 50, 0, d.screen().root_depth)
c1.map(); c2.map(); c3.map()
d.sync()

# Query initial stacking (QueryTree)
tree = parent.query_tree()
initial_order = [w.id for w in tree.children]
print(f"initial_count={len(initial_order)}")

# CirculateWindow: RaiseLowest (0) - bring bottom child to top.
# python-xlib exposes the protocol request as Window.circulate(direction);
# circulate_window does not exist on the Window object.
parent.circulate(X.RaiseLowest)
d.sync()

tree2 = parent.query_tree()
new_order = [w.id for w in tree2.children]
# The lowest child should now be at the top
print(f"circulated_count={len(new_order)}")

parent.destroy()
d.close()
