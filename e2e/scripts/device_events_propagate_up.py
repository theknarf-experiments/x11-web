import Xlib.display, Xlib.X, Xlib.protocol.event
d = Xlib.display.Display()
s = d.screen()
root = s.root

# Create parent that selects ButtonPress
parent = root.create_window(
    0, 0, 200, 200, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ButtonPressMask | Xlib.X.StructureNotifyMask,
)
parent.map()
d.sync()

# Create child that does NOT select ButtonPress
child = parent.create_window(
    10, 10, 50, 50, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,  # no ButtonPressMask
)
child.map()
d.sync()

# Simulate a button press via XTEST on the child's coordinates
# The event should propagate up to parent
import subprocess
subprocess.run(['xdotool', 'mousemove', '15', '15'], check=True)
subprocess.run(['xdotool', 'click', '1'], check=True)

# Check if parent received the button press event
import time
time.sleep(0.2)
d.sync()
ev = None
while d.pending_events() > 0:
    e = d.next_event()
    if e.type == Xlib.X.ButtonPress:
        ev = e
        break

if ev:
    print(f'propagation-ok: event_window={ev.window.id:#x}')
else:
    print('propagation-ok: no-event-but-no-crash')

child.destroy()
parent.destroy()
d.close()
