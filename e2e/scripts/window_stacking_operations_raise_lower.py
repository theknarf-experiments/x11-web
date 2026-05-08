import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Create two overlapping windows
w1 = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w2 = root.create_window(50, 50, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w1.map()
w2.map()
d.sync()

# Raise w1 above w2
w1.raise_window()
d.sync()

# Lower w1 below w2
w1.configure(stack_mode=Xlib.X.Below)
d.sync()

# Query the tree to check stacking order
tree = root.query_tree()
children = tree.children
if w1 in children and w2 in children:
    i1 = children.index(w1)
    i2 = children.index(w2)
    if i1 < i2:
        print("STACKING_OK")
    else:
        print(f"STACKING_FAIL: w1 at {i1}, w2 at {i2}")
else:
    print("STACKING_FAIL: windows not found in tree")
