import Xlib.display, Xlib.X, Xlib.protocol.event
d = Xlib.display.Display()
root = d.screen().root

w = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.PropertyChangeMask|Xlib.X.ExposureMask)
w.map()
d.sync()

# Send a valid synthetic Expose event (type=12)
event = Xlib.protocol.event.Expose(
    window=w,
    x=0, y=0, width=100, height=100, count=0)
w.send_event(event, event_mask=Xlib.X.ExposureMask)
d.sync()
print("SEND_EVENT_OK")

w.destroy()
d.sync()
