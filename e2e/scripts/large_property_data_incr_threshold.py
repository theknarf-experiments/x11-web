import Xlib.display, Xlib.X, Xlib.Xatom

d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set a large property (100KB - should work with or without INCR)
large_data = b'A' * 100000
atom = d.intern_atom('_LARGE_PROP_TEST')
w.change_property(atom, Xlib.Xatom.STRING, 8, large_data)
d.sync()

# Read it back
prop = w.get_full_property(atom, Xlib.Xatom.STRING)
if prop and len(prop.value) == 100000 and prop.value == large_data:
    print("LARGE_PROP_OK")
else:
    print(f"FAIL: got {len(prop.value) if prop else 0} bytes")

d.close()
