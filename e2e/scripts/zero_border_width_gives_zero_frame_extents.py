from Xlib import display, X
from Xlib.protocol import event
d = display.Display()
root = d.screen().root

w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth,
    event_mask=X.PropertyChangeMask)
d.sync()

net_request = d.intern_atom('_NET_REQUEST_FRAME_EXTENTS')
net_frame = d.intern_atom('_NET_FRAME_EXTENTS')

e = event.ClientMessage(window=w.id, client_type=net_request, data=(32, [0, 0, 0, 0, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

import time
time.sleep(0.1)
d.sync()
prop = w.get_full_property(net_frame, X.AnyPropertyType)
if prop and prop.value:
    import array
    vals = array.array('I', prop.value)
    print(f"all_zero={all(v == 0 for v in vals)}")
else:
    print("all_zero=True")

w.destroy()
d.close()
