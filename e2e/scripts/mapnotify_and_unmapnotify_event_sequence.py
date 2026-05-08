import Xlib.display, Xlib.X
import time

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)

# Map
w.map()
d.sync()
time.sleep(0.2)

events = []
while d.pending_events():
    e = d.next_event()
    events.append(e.type)

has_map = Xlib.X.MapNotify in events
print(f"map_notify={has_map}")

# Unmap
w.unmap()
d.sync()
time.sleep(0.2)

events2 = []
while d.pending_events():
    e = d.next_event()
    events2.append(e.type)

has_unmap = Xlib.X.UnmapNotify in events2
print(f"unmap_notify={has_unmap}")

w.destroy()
d.close()
