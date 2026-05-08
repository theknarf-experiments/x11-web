from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)
w.map()
d.sync()

prop = d.intern_atom("_TEST_RAPID")
for i in range(1000):
    w.change_property(prop, Xatom.STRING, 8, f"value_{i}".encode())
d.sync()

# Read final value
p = w.get_property(prop, Xatom.STRING, 0, 100)
val = p.value.decode() if p and p.value else "EMPTY"
print(f"final_value={val}")
w.destroy()
d.close()
