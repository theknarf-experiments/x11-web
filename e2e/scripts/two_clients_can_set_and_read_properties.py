import Xlib.display, Xlib.X, struct

# Client 1: create window and set property
d1 = Xlib.display.Display()
screen = d1.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d1.sync()

# Set a custom property
atom = d1.intern_atom('_TEST_MULTI_CLIENT')
w.change_property(atom, Xlib.Xatom.STRING, 8, b'hello_from_client1')
d1.sync()
wid = w.id

# Client 2: read the property
d2 = Xlib.display.Display()
w2 = d2.create_resource_object('window', wid)
prop = w2.get_full_property(d2.intern_atom('_TEST_MULTI_CLIENT'), Xlib.Xatom.STRING)
if prop and prop.value == b'hello_from_client1':
    print("MULTI_CLIENT_OK")
else:
    print(f"FAIL: prop={prop}")

d1.close()
d2.close()
