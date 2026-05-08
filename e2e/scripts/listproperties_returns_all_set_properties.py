import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth)
d.sync()

a1 = d.intern_atom('_LP_TEST_1')
a2 = d.intern_atom('_LP_TEST_2')
w.change_property(a1, Xlib.Xatom.STRING, 8, b'v1')
w.change_property(a2, Xlib.Xatom.STRING, 8, b'v2')
d.sync()

props = w.list_properties()
atom_ids = [p for p in props]
print(f"has_a1={a1 in atom_ids}")
print(f"has_a2={a2 in atom_ids}")
print(f"prop_count={len(atom_ids)}")

w.destroy()
d.close()
