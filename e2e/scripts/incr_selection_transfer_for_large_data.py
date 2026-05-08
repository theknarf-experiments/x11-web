import Xlib.display, Xlib.X, Xlib.Xatom
import time

d = Xlib.display.Display()
screen = d.screen()

# Test large property transfer
w = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth)
d.sync()

# Write a large property value (256KB)
large_data = b'X' * (256 * 1024)
test_atom = d.intern_atom('_LARGE_PROP_TEST')
w.change_property(test_atom, Xlib.Xatom.STRING, 8, large_data)
d.sync()

# Read it back
prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
if prop:
    read_len = len(prop.value)
    correct = read_len == len(large_data)
    print(f"written={len(large_data)}")
    print(f"read_back={read_len}")
    print(f"large_prop_ok={correct}")
else:
    print("large_prop_ok=False")

w.destroy()
d.close()
