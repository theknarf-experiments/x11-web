from Xlib import display, X
d = display.Display()
root = d.screen().root

parent = root.create_window(10, 10, 400, 400, 0, d.screen().root_depth)
parent.map()
d.sync()

child = root.create_window(50, 50, 200, 200, 0, d.screen().root_depth)
wm_transient_for = d.intern_atom('WM_TRANSIENT_FOR')
child.change_property(wm_transient_for, 33, 32, [parent.id])
child.map()
d.sync()

# Query tree to check stacking order
tree = root.query_tree()
children = tree.children
parent_idx = -1
child_idx = -1
for i, c in enumerate(children):
    if c.id == parent.id:
        parent_idx = i
    if c.id == child.id:
        child_idx = i

print(f"child_above_parent={child_idx > parent_idx}")

child.destroy()
parent.destroy()
d.close()
