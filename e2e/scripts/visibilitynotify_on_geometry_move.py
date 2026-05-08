"""
Confirms that the server emits `VisibilityNotify` when a
`ConfigureWindow` move uncovers a sibling — not just on stacking
changes. Both windows subscribe to `VisibilityChangeMask`; we move
the front window aside and wait for the events to arrive.
"""

import time

import Xlib.display
import Xlib.protocol.event  # noqa: F401  (registered via import side effect)
import Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Two overlapping windows.
win1 = screen.root.create_window(
    0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.VisibilityChangeMask | Xlib.X.StructureNotifyMask,
)
win2 = screen.root.create_window(
    50, 50, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.VisibilityChangeMask | Xlib.X.StructureNotifyMask,
)

win1.map()
win2.map()
d.sync()

time.sleep(0.2)

# Drain pending events from the initial map.
while d.pending_events():
    d.next_event()

# Move win2 away from win1 so it no longer overlaps.
win2.configure(x=300, y=300)
d.sync()
time.sleep(0.2)

vis_events = []
while d.pending_events():
    ev = d.next_event()
    if ev.type == Xlib.X.VisibilityNotify:
        vis_events.append(ev)

if len(vis_events) > 0:
    print(f"PASS: received {len(vis_events)} VisibilityNotify event(s) on geometry change")
else:
    print("FAIL: no VisibilityNotify on geometry change")

d.close()
