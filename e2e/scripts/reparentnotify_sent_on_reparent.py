import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
parent.map()

child = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
child.map()
d.sync()
while d.pending_events() > 0:
    d.next_event()

child.reparent(parent, 10, 10)
d.sync()

got_reparent = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.ReparentNotify:
        got_reparent = True
        break

if got_reparent:
    print("REPARENT_NOTIFY_OK")
else:
    print("NO_REPARENT_NOTIFY")

d.close()
