import Xlib.display, Xlib.X, Xlib.protocol.event
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(10, 10, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=0xFFFFFF)
w.map()
d.sync()

# Send a synthetic ClientMessage to the window
ev = Xlib.protocol.event.ClientMessage(
    window=w.id,
    client_type=d.intern_atom('TEST_ATOM'),
    data=(32, [1, 2, 3, 4, 5])
)
w.send_event(ev, event_mask=0)
d.sync()
print("send_event_ok=True")

w.destroy()
d.close()
