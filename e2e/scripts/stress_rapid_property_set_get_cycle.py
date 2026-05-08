import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth)

test_atom = d.intern_atom('_STRESS_TEST')
for i in range(200):
    data = f'value_{i}'.encode()
    w.change_property(test_atom, Xlib.Xatom.STRING, 8, data)
    d.sync()
    prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
    assert prop.value == data, f"Mismatch at {i}"

print("stress_ok=True")
w.destroy()
d.close()
