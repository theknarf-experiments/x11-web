import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display(":99")
screen = d.screen()
root = screen.root
# Create a window
w = root.create_window(10, 20, 200, 150, 2, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask | Xlib.X.StructureNotifyMask,
    background_pixel=screen.white_pixel)
d.sync()
# Get attributes before mapping
attrs = w.get_attributes()
assert attrs.map_state == 0, f"Should be unmapped, got {attrs.map_state}"
# Window depth is on GetGeometry, not GetWindowAttributes
geom_pre = w.get_geometry()
print(f"DEPTH={geom_pre.depth}")
# Map the window
w.map()
d.sync()
# Verify geometry
geom = w.get_geometry()
print(f"GEOM={geom.x},{geom.y},{geom.width},{geom.height},{geom.border_width}")
assert geom.width == 200, f"width mismatch: {geom.width}"
assert geom.height == 150, f"height mismatch: {geom.height}"
assert geom.border_width == 2, f"border mismatch: {geom.border_width}"
# QueryTree
tree = root.query_tree()
print(f"CHILDREN_COUNT={len(tree.children)}")
# tree.children is a list of Window resources; compare by id
child_ids = [c.id for c in tree.children]
assert w.id in child_ids, "Window not in root children"
# Destroy
w.destroy()
d.sync()
print("WINDOW_LIFECYCLE_OK")
d.close()
