import Xlib.display, Xlib.X, sys
d = Xlib.display.Display()
root = d.screen().root
# Create, map, configure, query, unmap, destroy cycle
w = root.create_window(10, 20, 300, 200, 2, d.screen().root_depth,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.ExposureMask)
w.map()
d.sync()
# GetGeometry round-trip
g = w.get_geometry()
assert g.width == 300, f"width {g.width}"
assert g.height == 200, f"height {g.height}"
# GetWindowAttributes round-trip
a = w.get_attributes()
assert a.map_state == 2, f"map_state {a.map_state}"  # IsViewable
# ConfigureWindow
w.configure(width=400, height=300)
d.sync()
g2 = w.get_geometry()
assert g2.width == 400, f"configured width {g2.width}"
# QueryTree
tree = root.query_tree()
assert w.id in [c.id for c in tree.children], "window not in tree"
# ReparentWindow test
w2 = root.create_window(0, 0, 50, 50, 0, d.screen().root_depth)
w2.reparent(w, 5, 5)
d.sync()
tree2 = w.query_tree()
assert w2.id in [c.id for c in tree2.children], "reparent failed"
# Unmap and verify
w.unmap()
d.sync()
a2 = w.get_attributes()
assert a2.map_state == 0, f"unmap state {a2.map_state}"  # IsUnmapped
w2.destroy()
w.destroy()
d.close()
print("xcb-lifecycle-ok")
