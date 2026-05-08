import Xlib.display, Xlib.X, time
d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w2 = screen.root.create_window(50, 50, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w1.map()
w2.map()
d.sync()
time.sleep(0.3)

tree = screen.root.query_tree()
children = list(tree.children)
w1_idx = children.index(w1) if w1 in children else -1
w2_idx = children.index(w2) if w2 in children else -1
print(f"w2_above_w1={w2_idx > w1_idx}")

w1.raise_window()
d.sync()
time.sleep(0.3)

tree2 = screen.root.query_tree()
children2 = list(tree2.children)
w1_idx2 = children2.index(w1) if w1 in children2 else -1
w2_idx2 = children2.index(w2) if w2 in children2 else -1
print(f"after_raise_w1_above_w2={w1_idx2 > w2_idx2}")

w1.destroy()
w2.destroy()
d.close()
