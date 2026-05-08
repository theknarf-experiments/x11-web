from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root
cb0 = d.intern_atom("CUT_BUFFER0")
cb1 = d.intern_atom("CUT_BUFFER1")
cb2 = d.intern_atom("CUT_BUFFER2")
# Set distinct values
root.change_property(cb0, Xatom.STRING, 8, b"zero")
root.change_property(cb1, Xatom.STRING, 8, b"one")
root.change_property(cb2, Xatom.STRING, 8, b"two")
d.sync()
# Rotate by 1: cb0->cb1, cb1->cb2, cb2->cb0
root.rotate_properties([cb0, cb1, cb2], 1)
d.sync()
# Read back
p0 = root.get_property(cb0, Xatom.STRING, 0, 100)
p1 = root.get_property(cb1, Xatom.STRING, 0, 100)
p2 = root.get_property(cb2, Xatom.STRING, 0, 100)
val0 = p0.value.decode() if p0 and p0.value else "EMPTY"
val1 = p1.value.decode() if p1 and p1.value else "EMPTY"
val2 = p2.value.decode() if p2 and p2.value else "EMPTY"
print(f"cb0={val0} cb1={val1} cb2={val2}")
d.close()
