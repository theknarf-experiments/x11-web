import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()
# Drain events
while d.pending_events() > 0:
    d.next_event()

# Destroy window
w.destroy()
d.sync()
# Events are async — give the server a moment to deliver before checking
import time; time.sleep(0.3)

got_destroy = False
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type == Xlib.X.DestroyNotify:
        got_destroy = True
        break

if got_destroy:
    print("DESTROY_NOTIFY_OK")
else:
    print("NO_DESTROY_NOTIFY")

d.close()
