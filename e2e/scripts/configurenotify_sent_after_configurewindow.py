import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    override_redirect=True,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()
while d.pending_events() > 0:
    d.next_event()

# Configure the window
w.configure(x=50, y=50, width=200, height=150)
d.sync()

got_configure = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.ConfigureNotify:
        if ev.width == 200 and ev.height == 150:
            got_configure = True
            break

if got_configure:
    print("CONFIGURE_NOTIFY_OK")
else:
    print("NO_CONFIGURE_NOTIFY")

d.close()
