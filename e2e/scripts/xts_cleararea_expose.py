import Xlib.display, Xlib.X, Xlib.protocol.event, sys, time
passed = 0; failed = 0
d = Xlib.display.Display()
s = d.screen()
w = s.root.create_window(10, 10, 100, 100, 0, s.root_depth,
    event_mask=Xlib.X.ExposureMask | Xlib.X.StructureNotifyMask)
w.map()
d.sync()
# Drain any pending events from MapNotify etc.
time.sleep(0.2)
while d.pending_events():
    d.next_event()
# ClearArea with exposures=True
w.clear_area(0, 0, 100, 100, exposures=True)
d.sync()
time.sleep(0.2)
# Check if we got an Expose event
got_expose = False
for _ in range(50):
    if d.pending_events():
        ev = d.next_event()
        if ev.type == Xlib.X.Expose:
            got_expose = True; break
    else:
        break
if got_expose:
    passed += 1; print("PASS: ClearArea exposures generated Expose")
else:
    failed += 1; print("FAIL: No Expose event from ClearArea")
w.destroy()
d.close()
print(f"xts-cleararea: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
