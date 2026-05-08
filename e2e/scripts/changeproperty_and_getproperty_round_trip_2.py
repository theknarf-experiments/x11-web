import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth)

# Set a string property
test_atom = d.intern_atom('_TEST_PROP')
w.change_property(test_atom, Xlib.Xatom.STRING, 8, b'hello world')
d.sync()

# Read it back
prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
if prop:
    print(f"value={prop.value.decode()}")
else:
    print("value=NONE")

w.destroy()
d.close()
