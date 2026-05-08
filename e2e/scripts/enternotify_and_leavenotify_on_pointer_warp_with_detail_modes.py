import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create two sibling windows
w1 = screen.root.create_window(10, 10, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
w2 = screen.root.create_window(200, 10, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
w1.map()
w2.map()
d.sync()

# Warp pointer into w1 (absolute, relative to root)
screen.root.warp_pointer(50, 50)
d.sync()
import time; time.sleep(0.1)

# Warp pointer into w2 (absolute, relative to root)
screen.root.warp_pointer(250, 50)
d.sync()
time.sleep(0.1)
d.sync()

# Check for crossing events
events_found = 0
while d.pending_events():
    ev = d.next_event()
    if ev.type in (Xlib.X.EnterNotify, Xlib.X.LeaveNotify):
        events_found += 1

print(f"crossing_events_generated={events_found > 0}")

w1.destroy()
w2.destroy()
d.close()
