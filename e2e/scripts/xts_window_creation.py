import Xlib.display, Xlib.X, Xlib.Xatom, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

# Test 1: CreateWindow succeeds
try:
    w = root.create_window(10, 10, 200, 150, 0,
        d.screen().root_depth,
        Xlib.X.InputOutput,
        Xlib.X.CopyFromParent)
    passed += 1; print(f"PASS: CreateWindow id=0x{w.id:x}")
except Exception as e:
    failed += 1; print(f"FAIL: CreateWindow: {e}")
    sys.exit(1)

# Test 2: GetWindowAttributes
try:
    attrs = w.get_attributes()
    if attrs.map_state == Xlib.X.IsUnmapped:
        passed += 1; print("PASS: new window is unmapped")
    else:
        failed += 1; print(f"FAIL: map_state={attrs.map_state}")
except Exception as e:
    failed += 1; print(f"FAIL: GetWindowAttributes: {e}")

# Test 3: MapWindow
try:
    w.map()
    d.sync()
    attrs = w.get_attributes()
    if attrs.map_state == Xlib.X.IsViewable:
        passed += 1; print("PASS: window is viewable after map")
    else:
        failed += 1; print(f"FAIL: map_state={attrs.map_state} after map")
except Exception as e:
    failed += 1; print(f"FAIL: MapWindow: {e}")

# Test 4: GetGeometry returns correct size
try:
    geom = w.get_geometry()
    if geom.width == 200 and geom.height == 150:
        passed += 1; print(f"PASS: geometry {geom.width}x{geom.height}")
    else:
        failed += 1; print(f"FAIL: geometry {geom.width}x{geom.height}, expected 200x150")
except Exception as e:
    failed += 1; print(f"FAIL: GetGeometry: {e}")

# Test 5: ConfigureWindow (resize)
try:
    w.configure(width=300, height=200)
    d.sync()
    geom = w.get_geometry()
    if geom.width == 300 and geom.height == 200:
        passed += 1; print("PASS: resize to 300x200")
    else:
        failed += 1; print(f"FAIL: after resize: {geom.width}x{geom.height}")
except Exception as e:
    failed += 1; print(f"FAIL: ConfigureWindow: {e}")

# Test 6: Create a child window
try:
    child = w.create_window(5, 5, 50, 50, 1,
        d.screen().root_depth,
        Xlib.X.InputOutput,
        Xlib.X.CopyFromParent)
    child.map()
    d.sync()
    passed += 1; print(f"PASS: child window id=0x{child.id:x}")
except Exception as e:
    failed += 1; print(f"FAIL: child window: {e}")

# Test 7: QueryTree
try:
    tree = w.query_tree()
    children = tree.children
    if len(children) >= 1:
        passed += 1; print(f"PASS: QueryTree shows {len(children)} child(ren)")
    else:
        failed += 1; print(f"FAIL: QueryTree children={len(children)}")
except Exception as e:
    failed += 1; print(f"FAIL: QueryTree: {e}")

# Test 8: UnmapWindow
try:
    w.unmap()
    d.sync()
    attrs = w.get_attributes()
    if attrs.map_state == Xlib.X.IsUnmapped:
        passed += 1; print("PASS: window unmapped")
    else:
        failed += 1; print(f"FAIL: map_state={attrs.map_state} after unmap")
except Exception as e:
    failed += 1; print(f"FAIL: UnmapWindow: {e}")

# Test 9: DestroyWindow (child)
try:
    child.destroy()
    d.sync()
    tree = w.query_tree()
    if len(tree.children) == 0:
        passed += 1; print("PASS: child destroyed, QueryTree empty")
    else:
        failed += 1; print(f"FAIL: children after destroy: {len(tree.children)}")
except Exception as e:
    failed += 1; print(f"FAIL: DestroyWindow: {e}")

# Test 10: DestroyWindow (parent)
try:
    w.destroy()
    d.sync()
    passed += 1; print("PASS: parent window destroyed")
except Exception as e:
    failed += 1; print(f"FAIL: destroy parent: {e}")

d.close()
print(f"xts-window: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
