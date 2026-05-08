import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()

# Check for MapNotify event
got_map_notify = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.MapNotify:
        got_map_notify = True
        break

if got_map_notify:
    print("MAP_NOTIFY_OK")
else:
    print("NO_MAP_NOTIFY")

d.close()
