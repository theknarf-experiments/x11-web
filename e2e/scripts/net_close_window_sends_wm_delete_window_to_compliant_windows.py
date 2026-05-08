import Xlib.display, Xlib.X, Xlib.protocol.event
import time
d = Xlib.display.Display()
screen = d.screen()

# Create window that supports WM_DELETE_WINDOW
w = screen.root.create_window(10, 10, 200, 200, 0, screen.root_depth)
protocols_atom = d.intern_atom('WM_PROTOCOLS')
delete_atom = d.intern_atom('WM_DELETE_WINDOW')
w.change_property(protocols_atom, d.intern_atom('ATOM'), 32, [delete_atom])
w.map()
d.sync()

# Send _NET_CLOSE_WINDOW to root
close_atom = d.intern_atom('_NET_CLOSE_WINDOW')
event = Xlib.protocol.event.ClientMessage(
    window=w,
    client_type=close_atom,
    data=(32, [0, 0, 0, 0, 0])
)
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()

# Check for the WM_DELETE_WINDOW ClientMessage
# Give server a moment to process
import select
d.fileno()
readable, _, _ = select.select([d.fileno()], [], [], 1.0)
if readable:
    count = d.pending_events()
    found = False
    for _ in range(count):
        e = d.next_event()
        if hasattr(e, 'client_type') and e.client_type == protocols_atom:
            found = True
            break
    print(f"result={'OK' if found else 'NO_DELETE_MSG'}")
else:
    print("result=NO_EVENTS")

w.destroy()
d.close()
