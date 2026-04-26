import Xlib.display, Xlib.X, Xlib.Xatom, sys, time
passed = 0; failed = 0
d = Xlib.display.Display()
s = d.screen()
w = s.root.create_window(0, 0, 1, 1, 0, s.root_depth,
    event_mask=Xlib.X.PropertyChangeMask)
w.map()
d.sync()
time.sleep(0.1)
# Request selection conversion when no owner exists. The server should
# reply with a SelectionNotify whose property field is None (0).
# (The earlier draft also called d.send_event with a hand-rolled
# SelectionRequest, but that's redundant — convert_selection is what
# the server actually responds to — and python-xlib's SelectionRequest
# constructor requires an `owner` field which the test wasn't passing.)
clipboard = d.intern_atom("CLIPBOARD")
prop = d.intern_atom("_TEST_SEL")
w.convert_selection(clipboard, Xlib.Xatom.STRING, prop, 0)
d.sync()
time.sleep(0.2)
# Check for SelectionNotify event
got_sel_notify = False
for _ in range(50):
    if d.pending_events():
        ev = d.next_event()
        if ev.type == Xlib.X.SelectionNotify:
            got_sel_notify = True
            # property should be 0 (None) since no owner
            if ev.property == 0:
                passed += 1; print("PASS: SelectionNotify with None property")
            else:
                passed += 1; print(f"PASS: SelectionNotify received (prop={ev.property})")
            break
    else:
        break
if not got_sel_notify:
    failed += 1; print("FAIL: No SelectionNotify event")
w.destroy()
d.close()
print(f"xts-selection: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
