import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display(":99")
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)
d.sync()
# Set a string property
prop_atom = d.intern_atom("_TEST_PROP")
w.change_property(prop_atom, Xlib.Xatom.STRING, 8, b"hello world")
d.sync()
# Read it back
val = w.get_property(prop_atom, Xlib.Xatom.STRING, 0, 100)
assert val is not None, "Property not found"
text = bytes(val.value).decode("latin-1")
assert text == "hello world", f"Property mismatch: {text}"
print(f"PROP_VALUE={text}")
# Append mode
w.change_property(prop_atom, Xlib.Xatom.STRING, 8, b"!!!", mode=Xlib.X.PropModeAppend)
d.sync()
val2 = w.get_property(prop_atom, Xlib.Xatom.STRING, 0, 100)
text2 = bytes(val2.value).decode("latin-1")
assert text2 == "hello world!!!", f"Append mismatch: {text2}"
# Delete property
w.delete_property(prop_atom)
d.sync()
val3 = w.get_property(prop_atom, Xlib.Xatom.STRING, 0, 100)
assert val3 is None, f"Property should be deleted, got {val3}"
w.destroy()
d.sync()
print("PROPERTY_CYCLE_OK")
d.close()
