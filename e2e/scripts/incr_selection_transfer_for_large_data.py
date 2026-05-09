import Xlib.display, Xlib.X, Xlib.Xatom
import time

d = Xlib.display.Display()
screen = d.screen()

# Test large property transfer — accumulate via repeated Append so the total
# stored data exceeds a single core X11 request (which is capped at 256KB
# unless BIG-REQUESTS is used; python-xlib doesn't auto-promote).
w = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth)
d.sync()

test_atom = d.intern_atom('_LARGE_PROP_TEST')

# Each chunk fits comfortably inside a single core X11 request.
chunk = b'X' * 32768
chunks = 8  # 256 KB total
w.change_property(test_atom, Xlib.Xatom.STRING, 8, chunk)
for _ in range(chunks - 1):
    w.change_property(test_atom, Xlib.Xatom.STRING, 8, chunk,
                      onerror=None, mode=Xlib.X.PropModeAppend)
d.sync()

expected_len = len(chunk) * chunks

# Read it back
prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
if prop:
    read_len = len(prop.value)
    correct = read_len == expected_len
    print(f"written={expected_len}")
    print(f"read_back={read_len}")
    print(f"large_prop_ok={correct}")
else:
    print("large_prop_ok=False")

w.destroy()
d.close()
