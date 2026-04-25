import Xlib.display
import Xlib.X
import Xlib.Xutil
import sys

errors = []

d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root

# Test 1: CreateWindow + GetGeometry round-trip
w = root.create_window(
    10, 20, 300, 200, 2,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    background_pixel=screen.white_pixel,
    event_mask=Xlib.X.StructureNotifyMask,
)
d.sync()

geom = w.get_geometry()
if geom.width != 300 or geom.height != 200:
    errors.append(f"GetGeometry size: {geom.width}x{geom.height} != 300x200")
else:
    print("PASS: CreateWindow + GetGeometry size correct")

if geom.border_width != 2:
    errors.append(f"GetGeometry border: {geom.border_width} != 2")
else:
    print("PASS: GetGeometry border_width correct")

# Test 2: GetWindowAttributes
attrs = w.get_attributes()
if attrs.map_state != Xlib.X.IsUnmapped:
    errors.append(f"Window should be unmapped, got map_state={attrs.map_state}")
else:
    print("PASS: new window is unmapped")

# Test 3: MapWindow + check map_state
w.map()
d.sync()
attrs = w.get_attributes()
if attrs.map_state == Xlib.X.IsUnmapped:
    errors.append("Window still unmapped after MapWindow")
else:
    print("PASS: MapWindow changes map_state")

# Test 4: ConfigureWindow (move + resize)
w.configure(x=50, y=60, width=400, height=300)
d.sync()
geom = w.get_geometry()
if geom.width != 400 or geom.height != 300:
    errors.append(f"ConfigureWindow size: {geom.width}x{geom.height} != 400x300")
else:
    print("PASS: ConfigureWindow resize works")

# Test 5: QueryTree
tree = root.query_tree()
if w.id not in [c.id for c in tree.children]:
    errors.append("QueryTree does not list our window")
else:
    print("PASS: QueryTree lists child window")

parent_tree = w.query_tree()
if parent_tree.parent.id != root.id:
    errors.append(f"QueryTree parent mismatch: {parent_tree.parent.id} != {root.id}")
else:
    print("PASS: QueryTree parent is root")

# Test 6: Child windows and QueryTree depth
child = w.create_window(
    5, 5, 50, 50, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    background_pixel=screen.black_pixel,
)
d.sync()

child_tree = child.query_tree()
if child_tree.parent.id != w.id:
    errors.append("Child parent should be w")
else:
    print("PASS: child QueryTree parent correct")

w_tree = w.query_tree()
if child.id not in [c.id for c in w_tree.children]:
    errors.append("QueryTree missing child window")
else:
    print("PASS: parent QueryTree lists child")

# Test 7: UnmapWindow
w.unmap()
d.sync()
attrs = w.get_attributes()
if attrs.map_state != Xlib.X.IsUnmapped:
    errors.append(f"UnmapWindow: map_state={attrs.map_state}")
else:
    print("PASS: UnmapWindow works")

# Test 8: DestroyWindow (child should be destroyed too)
child_id = child.id
w.destroy()
d.sync()

# Attempting to query the destroyed window should fail
try:
    # Use a raw resource object to avoid python-xlib caching
    from Xlib.xobject.drawable import Window as XWindow
    dead = XWindow(d.display, child_id)
    dead.get_geometry()
    errors.append("GetGeometry on destroyed child should have raised")
except Exception:
    print("PASS: DestroyWindow destroys children recursively")

# Test 9: CreateWindow with InputOnly class
input_only = root.create_window(
    0, 0, 100, 100, 0,
    0,  # depth must be 0 for InputOnly
    Xlib.X.InputOnly,
    Xlib.X.CopyFromParent,
    event_mask=Xlib.X.KeyPressMask,
)
d.sync()
input_only.map()
d.sync()
attrs = input_only.get_attributes()
if attrs.win_class != Xlib.X.InputOnly:
    errors.append(f"InputOnly window class: {attrs.win_class}")
else:
    print("PASS: InputOnly window created and mapped")
input_only.destroy()
d.sync()

d.close()

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("XTS_WINDOW_OK")
