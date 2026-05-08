from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root
# Write to CUT_BUFFER0 property on root
cut_buffer0 = d.intern_atom("CUT_BUFFER0")
root.change_property(cut_buffer0, Xatom.STRING, 8, b"test_cut_buffer_data")
d.sync()
# Read it back
prop = root.get_property(cut_buffer0, Xatom.STRING, 0, 100)
if prop and prop.value:
    print(f"cut_buffer0={prop.value.decode()}")
else:
    print("cut_buffer0=EMPTY")
d.close()
