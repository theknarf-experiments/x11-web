from Xlib import X, display
d = display.Display()
root = d.screen().root

# Parent selects SubstructureRedirectMask
parent = root.create_window(0, 0, 400, 400, 0, d.screen().root_depth,
    event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
parent.map()
d.sync()

# Create and map a child — should generate MapRequest (not MapNotify)
child = parent.create_window(10, 10, 50, 50, 0, d.screen().root_depth)
child.map()
d.sync()

import time
time.sleep(0.1)
got_map_request = False
while d.pending_events():
    ev = d.next_event()
    if ev.type == X.MapRequest:
        got_map_request = True
print(f"got_map_request={got_map_request}")

parent.destroy()
d.close()
