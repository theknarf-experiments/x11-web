import Xlib.display, Xlib.X, Xlib.Xatom, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
# Test 1: CreateWindow + GetGeometry (geometry, not attributes —
# get_attributes() does not return width/height; that's GetGeometry).
w = root.create_window(10, 20, 200, 150, 2, 24, Xlib.X.InputOutput)
d.sync()
geom = w.get_geometry()
if geom.width == 200 and geom.height == 150:
    passed += 1
else:
    print(f"FAIL: expected 200x150, got {geom.width}x{geom.height}")
    failed += 1
# Test 2: MapWindow + UnmapWindow (use get_attributes for map_state)
w.map()
d.sync()
attrs2 = w.get_attributes()
if attrs2.map_state == 2:  # IsViewable
    passed += 1
else:
    print(f"FAIL: map_state={attrs2.map_state}, expected 2")
    failed += 1
w.unmap()
d.sync()
attrs3 = w.get_attributes()
if attrs3.map_state == 0:  # IsUnmapped
    passed += 1
else:
    print(f"FAIL: map_state={attrs3.map_state} after unmap")
    failed += 1
# Test 3: QueryTree
tree = root.query_tree()
# query_tree.root is a Window object — compare via .id.
if getattr(tree.root, "id", tree.root) == root.id:
    passed += 1
else:
    failed += 1
# Test 4: ChangeProperty + GetProperty round-trip
TEST_ATOM = d.intern_atom("_X11WEB_TEST_PROP")
w.change_property(TEST_ATOM, Xlib.Xatom.STRING, 8, b"hello world")
d.sync()
prop = w.get_full_property(TEST_ATOM, Xlib.Xatom.STRING)
if prop and bytes(prop.value) == b"hello world":
    passed += 1
else:
    print(f"FAIL: property read-back mismatch: {prop and bytes(prop.value)!r}")
    failed += 1
# Test 5: DeleteProperty
w.delete_property(TEST_ATOM)
d.sync()
prop2 = w.get_full_property(TEST_ATOM, Xlib.Xatom.STRING)
if prop2 is None:
    passed += 1
else:
    print(f"FAIL: property still exists after delete")
    failed += 1
# Test 6: DestroyWindow
w.destroy()
d.sync()
passed += 1  # No error = success
# Test 7: InternAtom + GetAtomName round-trip
atom_id = d.intern_atom("_X11WEB_ROUNDTRIP_TEST")
atom_name = d.get_atom_name(atom_id)
if atom_name == "_X11WEB_ROUNDTRIP_TEST":
    passed += 1
else:
    print(f"FAIL: atom name mismatch: {atom_name}")
    failed += 1
# Test 8: only_if_exists=True for nonexistent atom returns 0
noatom = d.intern_atom("_X11WEB_NONEXISTENT_ATOM_12345", True)
if noatom == 0:
    passed += 1
else:
    print(f"FAIL: only_if_exists should return 0, got {noatom}")
    failed += 1
print(f"deep-protocol: pass={passed} fail={failed}")
d.close()
sys.exit(1 if failed > 0 else 0)
