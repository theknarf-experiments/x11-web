import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

got_expose = False
for _ in range(10):
    if d.pending_events() > 0:
        ev = d.next_event()
        if ev.type == Xlib.X.Expose:
            got_expose = True
            break
    else:
        import time
        time.sleep(0.1)
        d.sync()

if got_expose:
    print("EXPOSE_ON_MAP_OK")
else:
    print("NO_EXPOSE_ON_MAP")

d.close()
