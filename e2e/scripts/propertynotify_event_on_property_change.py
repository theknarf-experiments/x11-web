import Xlib.display, Xlib.X, Xlib.Xatom
import time

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(
    0, 0, 50, 50, 0, screen.root_depth,
    event_mask=Xlib.X.PropertyChangeMask,
)
d.sync()

test_atom = d.intern_atom('_PROP_NOTIFY_TEST')
w.change_property(test_atom, Xlib.Xatom.STRING, 8, b'test_value')
d.sync()

time.sleep(0.3)

got_notify = False
while d.pending_events():
    ev = d.next_event()
    if ev.type == Xlib.X.PropertyNotify:
        if ev.atom == test_atom:
            got_notify = True
            # state 0 = PropertyNewValue
            print(f"notify_state={ev.state}")

print(f"got_property_notify={got_notify}")

w.destroy()
d.close()
