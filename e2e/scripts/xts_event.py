import Xlib.display
import Xlib.X
import Xlib.protocol.event
import sys
import time

errors = []

d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root

# Test 1: StructureNotifyMask delivers MapNotify/ConfigureNotify
w = root.create_window(
    0, 0, 200, 200, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    background_pixel=screen.white_pixel,
    event_mask=(Xlib.X.StructureNotifyMask |
                Xlib.X.PropertyChangeMask),
)
d.sync()

w.map()
d.sync()

# Drain events
found_map_notify = False
found_configure_notify = False
for _ in range(20):
    ev = d.pending_events()
    if ev == 0:
        d.sync()
        time.sleep(0.05)
        continue
    event = d.next_event()
    if event.type == Xlib.X.MapNotify:
        found_map_notify = True
    if event.type == Xlib.X.ConfigureNotify:
        found_configure_notify = True

if found_map_notify:
    print("PASS: MapNotify delivered")
else:
    errors.append("MapNotify not received")

# Test 2: PropertyChangeMask delivers PropertyNotify
test_atom = d.intern_atom('_XTS_EVENT_TEST')
w.change_property(test_atom, Xlib.Xatom.STRING, 8, b'test')
d.sync()

found_property_notify = False
for _ in range(20):
    ev = d.pending_events()
    if ev == 0:
        d.sync()
        time.sleep(0.05)
        continue
    event = d.next_event()
    if event.type == Xlib.X.PropertyNotify:
        found_property_notify = True
        if event.atom == test_atom:
            print("PASS: PropertyNotify has correct atom")
        else:
            errors.append(f"PropertyNotify atom mismatch: {event.atom} != {test_atom}")
        break

if found_property_notify:
    print("PASS: PropertyNotify delivered")
else:
    errors.append("PropertyNotify not received")

# Test 3: SubstructureNotifyMask on parent
parent = root.create_window(
    0, 0, 400, 400, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    background_pixel=screen.white_pixel,
    event_mask=Xlib.X.SubstructureNotifyMask,
)
parent.map()
d.sync()

# Create child - parent should get CreateNotify
child = parent.create_window(
    0, 0, 50, 50, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    background_pixel=screen.black_pixel,
)
d.sync()

found_create_notify = False
for _ in range(20):
    ev = d.pending_events()
    if ev == 0:
        d.sync()
        time.sleep(0.05)
        continue
    event = d.next_event()
    if event.type == Xlib.X.CreateNotify:
        found_create_notify = True
        break

if found_create_notify:
    print("PASS: CreateNotify delivered to parent")
else:
    errors.append("CreateNotify not delivered to parent")

# Test 4: SendEvent (synthetic events)
# Send a synthetic ClientMessage to our window
cm = Xlib.protocol.event.ClientMessage(
    window=w,
    client_type=test_atom,
    data=(32, [1, 2, 3, 4, 5]),
)
w.send_event(cm, event_mask=0)
d.sync()

found_client_message = False
for _ in range(20):
    ev = d.pending_events()
    if ev == 0:
        d.sync()
        time.sleep(0.05)
        continue
    event = d.next_event()
    if event.type == Xlib.X.ClientMessage:
        found_client_message = True
        if event.client_type == test_atom:
            print("PASS: SendEvent ClientMessage type correct")
        else:
            errors.append(f"ClientMessage type mismatch")
        break

if found_client_message:
    print("PASS: SendEvent delivers synthetic event")
else:
    errors.append("SendEvent ClientMessage not received")

# Test 5: Event mask filtering - window without mask should not get events
w2 = root.create_window(
    0, 0, 100, 100, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    background_pixel=screen.white_pixel,
    event_mask=0,  # No event mask
)
w2.map()
d.sync()

# Change property on w2 - should NOT generate PropertyNotify since mask is 0
prop_atom = d.intern_atom('_XTS_NO_EVENT_TEST')
w2.change_property(prop_atom, Xlib.Xatom.STRING, 8, b'test')
d.sync()
time.sleep(0.1)

# Drain any pending events
spurious_property = False
while d.pending_events():
    event = d.next_event()
    if event.type == Xlib.X.PropertyNotify and hasattr(event, 'window') and event.window.id == w2.id:
        spurious_property = True

if not spurious_property:
    print("PASS: event mask filtering works (no PropertyNotify without mask)")
else:
    errors.append("PropertyNotify received on window with mask=0")

# Cleanup
child.destroy()
parent.destroy()
w.destroy()
w2.destroy()
d.sync()
d.close()

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("XTS_EVENT_OK")
