from Xlib import display, X
import time

d = display.Display()
screen = d.screen()
root = screen.root

# Window WITH StructureNotifyMask
w1 = root.create_window(
    10, 10, 100, 100, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
    event_mask=X.StructureNotifyMask,
)
w1.map()
d.sync()
time.sleep(0.1)

got_map = False
while d.pending_events():
    ev = d.next_event()
    if ev.type == X.MapNotify:
        got_map = True

print(f"map_notify_with_mask={got_map}")
w1.destroy()
d.close()
