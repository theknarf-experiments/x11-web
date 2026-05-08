import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.SubstructureNotifyMask)
parent.map()
d.sync()

# Create a child — should generate CreateNotify on parent
child = parent.create_window(10, 10, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
d.sync()
import time; time.sleep(0.2)  # event delivery is async

got_create_notify = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.CreateNotify:
        got_create_notify = True
        break

if got_create_notify:
    print("CREATE_NOTIFY_OK")
else:
    print("NO_CREATE_NOTIFY")

d.close()
