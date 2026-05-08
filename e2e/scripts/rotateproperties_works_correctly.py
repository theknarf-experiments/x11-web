import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth)
d.sync()

a1 = d.intern_atom('_ROT_A')
a2 = d.intern_atom('_ROT_B')
a3 = d.intern_atom('_ROT_C')

w.change_property(a1, Xlib.Xatom.STRING, 8, b'val_a')
w.change_property(a2, Xlib.Xatom.STRING, 8, b'val_b')
w.change_property(a3, Xlib.Xatom.STRING, 8, b'val_c')
d.sync()

# Rotate by +1: a1->a2, a2->a3, a3->a1
w.rotate_properties([a1, a2, a3], 1)
d.sync()

# After rotation +1: a1 should have val_c, a2 should have val_a, a3 should have val_b
p1 = w.get_full_property(a1, Xlib.Xatom.STRING)
p2 = w.get_full_property(a2, Xlib.Xatom.STRING)
p3 = w.get_full_property(a3, Xlib.Xatom.STRING)

v1 = p1.value.decode() if p1 else "NONE"
v2 = p2.value.decode() if p2 else "NONE"
v3 = p3.value.decode() if p3 else "NONE"

print(f"a1={v1} a2={v2} a3={v3}")
# +1 rotation means each property gets the value of the previous one
# So: a1 gets a3's value (val_c), a2 gets a1's value (val_a), a3 gets a2's value (val_b)
print(f"rotate_ok={v1 == 'val_c' and v2 == 'val_a' and v3 == 'val_b'}")

w.destroy()
d.close()
