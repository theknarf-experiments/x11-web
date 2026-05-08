import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()
while d.pending_events() > 0:
    d.next_event()

w.unmap()
d.sync()

got_unmap = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.UnmapNotify:
        got_unmap = True
        break

if got_unmap:
    print("UNMAP_NOTIFY_OK")
else:
    print("NO_UNMAP_NOTIFY")

d.close()
