from Xlib import display, X
from Xlib.protocol import event
d = display.Display()
root = d.screen().root

w = root.create_window(10, 10, 200, 200, 2, d.screen().root_depth,
    event_mask=X.PropertyChangeMask)
d.sync()

net_request = d.intern_atom('_NET_REQUEST_FRAME_EXTENTS')
net_frame = d.intern_atom('_NET_FRAME_EXTENTS')

# Send _NET_REQUEST_FRAME_EXTENTS ClientMessage
e = event.ClientMessage(window=w.id, client_type=net_request, data=(32, [0, 0, 0, 0, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

# Read _NET_FRAME_EXTENTS
import time
time.sleep(0.1)
d.sync()
prop = w.get_full_property(net_frame, X.AnyPropertyType)
if prop and prop.value:
    import array
    vals = array.array('I', prop.value)
    print(f"extents={list(vals)}")
    # With border_width=2, all extents should be 2
    print(f"correct={all(v == 2 for v in vals)}")
else:
    print("extents=none")
    print("correct=False")

w.destroy()
d.close()
