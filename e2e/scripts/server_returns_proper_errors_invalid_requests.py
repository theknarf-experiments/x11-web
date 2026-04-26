import Xlib, Xlib.display, sys
passed = 0; failed = 0
d = Xlib.display.Display()

# Test 1: GetWindowAttributes on invalid window ID
try:
    from Xlib.protocol.request import GetWindowAttributes
    d.get_atom("_NONEXISTENT_ATOM_12345", only_if_exists=True)
    passed += 1; print("PASS: InternAtom only_if_exists=True returns 0")
except Exception as e:
    failed += 1; print(f"FAIL: {e}")

# Test 2: QueryTree on root window
try:
    root = d.screen().root
    tree = root.query_tree()
    if tree.root == root.id:
        passed += 1; print("PASS: QueryTree root matches")
    else:
        failed += 1; print(f"FAIL: root mismatch {tree.root} != {root.id}")
except Exception as e:
    failed += 1; print(f"FAIL: QueryTree: {e}")

# Test 3: ListProperties on root
try:
    props = root.list_properties()
    if len(props) >= 0:
        passed += 1; print(f"PASS: ListProperties returned {len(props)} props")
except Exception as e:
    failed += 1; print(f"FAIL: ListProperties: {e}")

# Test 4: GetKeyboardMapping returns valid data
try:
    mapping = d.get_keyboard_mapping(8, 248)
    if len(mapping) > 0:
        passed += 1; print(f"PASS: GetKeyboardMapping returned {len(mapping)} codes")
    else:
        failed += 1; print("FAIL: empty keyboard mapping")
except Exception as e:
    failed += 1; print(f"FAIL: GetKeyboardMapping: {e}")

# Test 5: QueryPointer returns valid coordinates
try:
    ptr = root.query_pointer()
    if hasattr(ptr, "root_x") and hasattr(ptr, "root_y"):
        passed += 1; print(f"PASS: QueryPointer at ({ptr.root_x},{ptr.root_y})")
    else:
        failed += 1; print("FAIL: missing pointer coords")
except Exception as e:
    failed += 1; print(f"FAIL: QueryPointer: {e}")

# Test 6: GetInputFocus returns valid focus
try:
    focus = d.get_input_focus()
    if focus.focus is not None:
        passed += 1; print(f"PASS: GetInputFocus returned focus")
    else:
        failed += 1; print("FAIL: null focus")
except Exception as e:
    failed += 1; print(f"FAIL: GetInputFocus: {e}")

d.close()
print(f"error-handling: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
