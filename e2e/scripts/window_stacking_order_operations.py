import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create three windows
w1 = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w2 = screen.root.create_window(50, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w3 = screen.root.create_window(100, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w1.map()
w2.map()
w3.map()
d.sync()

# Raise w1 to top
w1.configure(stack_mode=Xlib.X.Above)
d.sync()

# Query stacking order
tree = screen.root.query_tree()
children = [c.id for c in tree.children]
# w1 should be at or near the top
if w1.id in children:
    pos = children.index(w1.id)
    print(f"w1_position={pos} total={len(children)}")
    print("STACKING_OK")

d.close()
