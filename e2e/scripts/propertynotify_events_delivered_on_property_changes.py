from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root

w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    event_mask=X.PropertyChangeMask)
w.map()
d.sync()

# Set a property
test_atom = d.intern_atom("_TEST_PROP")
w.change_property(test_atom, Xatom.STRING, 8, b"hello")
d.sync()

# Check for PropertyNotify
import time
time.sleep(0.1)
count = 0
while d.pending_events():
    ev = d.next_event()
    if ev.type == X.PropertyNotify:
        count += 1
print(f"property_notify_count={count}")
d.close()
