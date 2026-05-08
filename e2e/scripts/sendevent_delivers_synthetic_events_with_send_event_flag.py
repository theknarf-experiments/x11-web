import Xlib.display, Xlib.X, Xlib.protocol.event
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask | Xlib.X.PropertyChangeMask)
w.map()
d.sync()

# Send a synthetic PropertyNotify
test_atom = d.intern_atom('_SEND_EVENT_TEST')
evt = Xlib.protocol.event.PropertyNotify(
    window=w,
    atom=test_atom,
    time=0,
    state=0,
)
w.send_event(evt, event_mask=Xlib.X.PropertyChangeMask)
d.sync()

import time
time.sleep(0.3)

# Check for the event
found = False
while d.pending_events():
    e = d.next_event()
    if e.type == Xlib.X.PropertyNotify and hasattr(e, 'send_event') and e.send_event:
        found = True
        break

print(f"synthetic_event_delivered={found}")

w.destroy()
d.close()
