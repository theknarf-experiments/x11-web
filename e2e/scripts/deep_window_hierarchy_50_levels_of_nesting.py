import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create a chain of 50 nested windows
windows = []
parent = screen.root
for i in range(50):
    w = parent.create_window(1, 1, max(200 - i*3, 10), max(200 - i*3, 10),
        0, screen.root_depth)
    w.map()
    windows.append(w)
    parent = w

d.sync()

# Verify the deepest window exists and has correct geometry
deepest = windows[-1]
geo = deepest.get_geometry()
print(f"deepest_width={geo.width}")

# Verify the tree
tree = windows[-2].query_tree()
child_ids = [c.id for c in tree.children]
print(f"deepest_in_parent={deepest.id in child_ids}")

# Destroy from innermost to outermost
for w in reversed(windows):
    w.destroy()
d.sync()

print("result=OK")
d.close()
