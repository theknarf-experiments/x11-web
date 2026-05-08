import Xlib.display, Xlib.X, Xlib.Xatom
import time, threading

selection_atom = None
target_atom = None
prop_atom = None

def owner_thread():
    """Connection A: owns the selection and responds to requests."""
    d1 = Xlib.display.Display()
    screen = d1.screen()
    global selection_atom, target_atom, prop_atom

    selection_atom = d1.intern_atom('_TEST_SELECTION')
    target_atom = d1.intern_atom('UTF8_STRING')
    prop_atom = d1.intern_atom('_TEST_SEL_PROP')

    w1 = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth)
    d1.sync()

    w1.set_selection_owner(selection_atom, Xlib.X.CurrentTime)
    d1.sync()

    owner = d1.get_selection_owner(selection_atom)
    print(f"owner_set={owner.id == w1.id}")

    # Wait for SelectionRequest event
    deadline = time.monotonic() + 5
    got_request = False
    while time.monotonic() < deadline:
        if d1.pending_events():
            ev = d1.next_event()
            if ev.type == Xlib.X.SelectionRequest:
                got_request = True
                print(f"got_selection_request=True")
                # Respond by setting the property and sending SelectionNotify
                from Xlib import protocol
                ev.requestor.change_property(
                    ev.property, target_atom, 8, b'hello_selection'
                )
                notify = protocol.event.SelectionNotify(
                    time=ev.time,
                    requestor=ev.requestor,
                    selection=ev.selection,
                    target=ev.target,
                    property=ev.property,
                )
                ev.requestor.send_event(notify, event_mask=0)
                d1.sync()
                break
        time.sleep(0.1)

    if not got_request:
        print("got_selection_request=False")

    w1.destroy()
    d1.close()

owner_t = threading.Thread(target=owner_thread)
owner_t.start()
time.sleep(0.5)  # let owner set up

# Connection B: request the selection
d2 = Xlib.display.Display()
screen2 = d2.screen()

sel_atom = d2.intern_atom('_TEST_SELECTION')
tgt_atom = d2.intern_atom('UTF8_STRING')
prp_atom = d2.intern_atom('_TEST_SEL_PROP')

w2 = screen2.root.create_window(0, 0, 1, 1, 0, screen2.root_depth)
d2.sync()

w2.convert_selection(sel_atom, tgt_atom, prp_atom, Xlib.X.CurrentTime)
d2.sync()

# Wait for SelectionNotify
deadline = time.monotonic() + 5
got_notify = False
while time.monotonic() < deadline:
    if d2.pending_events():
        ev = d2.next_event()
        if ev.type == Xlib.X.SelectionNotify:
            got_notify = True
            if ev.property != Xlib.X.NONE:
                prop = w2.get_full_property(prp_atom, tgt_atom)
                if prop:
                    print(f"selection_value={prop.value.decode()}")
            break
    time.sleep(0.1)

print(f"got_selection_notify={got_notify}")

w2.destroy()
d2.close()
owner_t.join(timeout=5)
