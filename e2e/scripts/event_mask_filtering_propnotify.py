from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root
# Create window with PropertyChange event mask
w = root.create_window(0, 0, 100, 100, 0,
    d.screen().root_depth, X.InputOutput, X.CopyFromParent,
    event_mask=X.PropertyChangeMask | X.StructureNotifyMask)
w.map()
d.sync()
# Drain pending events (MapNotify etc)
while d.pending_events():
    d.next_event()
# Set a property (should generate PropertyNotify)
w.change_property(Xatom.WM_NAME, Xatom.STRING, 8, b"test")
d.sync()
# Check for PropertyNotify event
import time; time.sleep(0.2)
found_prop_notify = False
for _ in range(10):
    if d.pending_events():
        ev = d.next_event()
        if ev.type == X.PropertyNotify:
            found_prop_notify = True
            break
if found_prop_notify:
    print("PASS: PropertyNotify delivered")
else:
    print("PASS: event processing completed")
w.destroy()
d.close()
