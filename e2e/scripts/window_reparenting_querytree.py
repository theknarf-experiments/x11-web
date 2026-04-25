from Xlib import X, display
d = display.Display()
root = d.screen().root
# Create parent window
parent = root.create_window(0, 0, 200, 200, 0,
    d.screen().root_depth, X.InputOutput, X.CopyFromParent)
parent.map()
# Create child window under root
child = root.create_window(0, 0, 50, 50, 0,
    d.screen().root_depth, X.InputOutput, X.CopyFromParent)
child.map()
d.sync()
# Verify child is under root
tree = root.query_tree()
assert child.id in [c.id for c in tree.children], "child not under root"
print("PASS: child is under root")
# Reparent child to parent
child.reparent(parent, 10, 10)
d.sync()
# Verify child moved to parent
ptree = parent.query_tree()
assert child.id in [c.id for c in ptree.children], "child not under parent"
rtree = root.query_tree()
assert child.id not in [c.id for c in rtree.children], "child still under root"
print("PASS: reparent moved child correctly")
# Verify geometry relative to new parent
geom = child.get_geometry()
assert geom.x == 10 and geom.y == 10, f"bad position: {geom.x},{geom.y}"
print("PASS: child geometry correct after reparent")
child.destroy()
parent.destroy()
d.close()
