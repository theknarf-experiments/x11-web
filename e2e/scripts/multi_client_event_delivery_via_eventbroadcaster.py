import Xlib.display, Xlib.X
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()
screen = d1.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.PropertyChangeMask | Xlib.X.StructureNotifyMask)
w.map()
d1.sync()

# Second client selects PropertyChangeMask on the same window
from Xlib.xobject.drawable import Window
w2 = Window(d2, w.id)
w2.change_attributes(event_mask=Xlib.X.PropertyChangeMask)
d2.sync()

# Change a property from client 1
test_atom = d1.intern_atom('_TEST_BROADCAST')
w.change_property(test_atom, Xlib.X.Xatom.STRING, 8, b'test')
d1.sync()

# Client 2 should get PropertyNotify
import time
time.sleep(0.1)
d2.sync()
ev_count = d2.pending_events()
print(f"client2_pending_events={ev_count}")
has_property_notify = False
while d2.pending_events() > 0:
    ev = d2.next_event()
    if ev.type == Xlib.X.PropertyNotify:
        has_property_notify = True
        break
print(f"client2_got_property_notify={has_property_notify}")

w.destroy()
d1.close()
d2.close()
