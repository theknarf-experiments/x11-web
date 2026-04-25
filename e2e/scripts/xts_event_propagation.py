import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

# Create parent -> child hierarchy
parent = root.create_window(0, 0, 200, 200, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ButtonPressMask | Xlib.X.SubstructureNotifyMask)
parent.map()
d.sync()

child = parent.create_window(10, 10, 50, 50, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=0)  # No event mask on child
child.map()
d.sync()

# Test 1: event mask inheritance - parent should get button events from child
# Use XSendEvent to simulate (we cannot warp+click atomically from python)
import Xlib.protocol.event
ev = Xlib.protocol.event.ButtonPress(
    time=Xlib.X.CurrentTime,
    root=root,
    window=child,
    child=Xlib.X.NONE,
    root_x=15, root_y=15,
    event_x=5, event_y=5,
    state=0, detail=1,
    same_screen=1)
child.send_event(ev, event_mask=0, propagate=True)
d.sync()

import time; time.sleep(0.1)
got_event = False
while d.pending_events():
    e = d.next_event()
    if e.type == Xlib.X.ButtonPress:
        got_event = True
        break

if got_event:
    passed += 1; print("PASS: ButtonPress propagated to parent")
else:
    failed += 1; print("FAIL: ButtonPress did not propagate")

# Test 2: do_not_propagate_mask blocks propagation
child.change_attributes(do_not_propagate_mask=Xlib.X.ButtonPressMask)
d.sync()

child.send_event(ev, event_mask=0, propagate=True)
d.sync()
time.sleep(0.1)
got_event2 = False
while d.pending_events():
    e = d.next_event()
    if e.type == Xlib.X.ButtonPress:
        got_event2 = True
        break

if not got_event2:
    passed += 1; print("PASS: do_not_propagate_mask blocks propagation")
else:
    failed += 1; print("FAIL: event propagated despite do_not_propagate_mask")

parent.destroy()
d.close()
print(f"xts-event-propagation: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
