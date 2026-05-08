import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
root = d.screen().root

w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set a large property (128KB)
large_data = bytes(range(256)) * 512  # 128KB
test_atom = d.intern_atom("_TEST_INCR_PROP")
w.change_property(test_atom, Xlib.Xatom.STRING, 8, large_data)
d.sync()

# Read it back with GetProperty (partial read, then delete)
prop = w.get_full_property(test_atom, Xlib.X.AnyPropertyType)
if prop and len(prop.value) == len(large_data):
    print("LARGE_PROP_OK")
else:
    got_len = len(prop.value) if prop else 0
    print(f"LARGE_PROP_FAIL: expected {len(large_data)}, got {got_len}")

# Delete the property
w.delete_property(test_atom)
d.sync()

# Verify deletion
prop2 = w.get_full_property(test_atom, Xlib.X.AnyPropertyType)
if prop2 is None:
    print("DELETE_PROP_OK")
else:
    print("DELETE_PROP_FAIL: property still exists")
