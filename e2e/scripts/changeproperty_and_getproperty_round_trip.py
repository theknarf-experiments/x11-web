from Xlib import display, X, Xatom
d = display.Display()
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth)
# Set a string property
test_atom = d.intern_atom('X11_WEB_TEST_PROP')
w.change_property(test_atom, Xatom.STRING, 8, b'hello world')
d.sync()
# Read it back
prop = w.get_full_property(test_atom, Xatom.STRING)
print(f"value={prop.value.decode() if prop else 'None'}")
print(f"format={prop.format if prop else 0}")
w.destroy()
d.close()
