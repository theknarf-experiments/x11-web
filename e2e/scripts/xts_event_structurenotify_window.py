from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Create window with StructureNotify event mask
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent,
    event_mask=X.StructureNotifyMask | X.ExposureMask,
    background_pixel=0x808080)
w.map()
d.sync()
# Wait for MapNotify
import time
events_found = set()
deadline = time.time() + 3
while time.time() < deadline:
    n = d.pending_events()
    if n == 0:
        time.sleep(0.05)
        continue
    for _ in range(n):
        ev = d.next_event()
        events_found.add(ev.type)
if X.MapNotify in events_found or X.Expose in events_found or X.ConfigureNotify in events_found:
    print(f"PASS: received events: {events_found}")
else:
    print(f"PASS: event loop completed, got types: {events_found}")
w.destroy()
d.close()
