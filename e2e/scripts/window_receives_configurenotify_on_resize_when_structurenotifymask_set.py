from Xlib import display, X, Xutil
import time, select

d = display.Display()
screen = d.screen()
root = screen.root

w = root.create_window(
    10, 10, 200, 200, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
    event_mask=X.StructureNotifyMask | X.ExposureMask,
)
w.map()
d.sync()

# Resize the window
w.configure(width=300, height=250)
d.sync()

# Check for ConfigureNotify
got_configure = False
for _ in range(50):
    while d.pending_events():
        ev = d.next_event()
        if ev.type == X.ConfigureNotify:
            if ev.width == 300 and ev.height == 250:
                got_configure = True
    if got_configure:
        break
    time.sleep(0.05)

print(f"configure_notify_received={got_configure}")
w.destroy()
d.close()
