import Xlib.display, Xlib.X, sys, time
passed = 0; failed = 0
d = Xlib.display.Display()
s = d.screen()
w = s.root.create_window(10, 10, 100, 100, 0, s.root_depth,
    event_mask=Xlib.X.ExposureMask | Xlib.X.StructureNotifyMask)
w.map()
d.sync()
time.sleep(0.3)
# Drain pending events
while d.pending_events():
    d.next_event()
# Resize the window
w.configure(width=200, height=200)
d.sync()
time.sleep(0.3)
# Check for Expose event
got_expose = False
got_configure = False
for _ in range(50):
    if d.pending_events():
        ev = d.next_event()
        if ev.type == Xlib.X.Expose:
            got_expose = True
        if ev.type == Xlib.X.ConfigureNotify:
            got_configure = True
    else:
        break
if got_configure:
    passed += 1; print("PASS: ConfigureNotify received")
else:
    failed += 1; print("FAIL: No ConfigureNotify")
if got_expose:
    passed += 1; print("PASS: Expose on resize received")
else:
    failed += 1; print("FAIL: No Expose on resize")
w.destroy()
d.close()
print(f"xts-resize-expose: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
