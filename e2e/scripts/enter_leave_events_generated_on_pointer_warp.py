import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(100, 100, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
w.map()
d.sync()

# Warp into the window (use w.warp_pointer for absolute coordinates)
w.warp_pointer(50, 50)  # warp to (50,50) relative to w = (150,150) absolute
d.sync()

import time
time.sleep(0.2)
d.sync()

events = []
while d.pending_events() > 0:
    ev = d.next_event()
    events.append(ev.type)

# EnterNotify=7, LeaveNotify=8
print(f"event_types={events}")
print(f"got_enter={7 in events}")

w.destroy()
d.close()
