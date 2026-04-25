import Xlib.display
import Xlib.X
import sys

d = Xlib.display.Display(':99')
root = d.screen().root
errors = []

# Create a deep window hierarchy (64 levels)
depth = 64
windows = [root]
try:
    parent = root
    for i in range(depth):
        w = parent.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
                                window_class=Xlib.X.InputOutput)
        w.map()
        windows.append(w)
        parent = w
    d.sync()
    print(f"PASS: created {depth}-deep window hierarchy")
except Exception as e:
    errors.append(f"deep hierarchy: {e}")

# Query geometry of the deepest window
try:
    deepest = windows[-1]
    geom = deepest.get_geometry()
    print(f"PASS: GetGeometry on depth-{depth} window: {geom.width}x{geom.height}")
except Exception as e:
    errors.append(f"GetGeometry on deep window: {e}")

# QueryTree should work on deep windows
try:
    tree = windows[-1].query_tree()
    print(f"PASS: QueryTree on depth-{depth} window: parent={tree.parent.id:#x}")
except Exception as e:
    errors.append(f"QueryTree on deep window: {e}")

# Clean up
for w in reversed(windows[1:]):
    try:
        w.destroy()
    except:
        pass
d.sync()
d.close()

if errors:
    print(f"FAIL: {errors}")
    sys.exit(1)
print("DEEP_HIERARCHY_OK")
