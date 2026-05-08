import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)
w2 = screen.root.create_window(50, 50, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)
w1.map()
w2.map()
d.sync()

# Drain initial events
import time
time.sleep(0.1)
while d.pending_events() > 0:
    d.next_event()

# Raise w1 above w2
w1.configure(stack_mode=Xlib.X.Above)
d.sync()
time.sleep(0.1)

got_configure = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == 22:  # ConfigureNotify
        got_configure = True

print(f"got_configure_notify={got_configure}")

w1.destroy()
w2.destroy()
d.close()
