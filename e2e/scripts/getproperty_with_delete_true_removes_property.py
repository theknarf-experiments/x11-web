import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth)
test_atom = d.intern_atom('_DELETE_TEST')

# Set property
w.change_property(test_atom, Xlib.Xatom.STRING, 8, b'delete_me')
d.sync()

# Verify it exists
prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
print(f"before_delete={prop is not None}")

# Get with delete=True
prop2 = w.get_property(test_atom, Xlib.Xatom.STRING, 0, 100, True)
d.sync()

# Property should be gone now
prop3 = w.get_full_property(test_atom, Xlib.Xatom.STRING)
print(f"after_delete={prop3 is None}")

w.destroy()
d.close()
