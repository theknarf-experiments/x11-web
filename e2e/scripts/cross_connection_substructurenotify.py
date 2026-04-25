import Xlib.display, Xlib.X, time
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()
root = d1.screen().root
# Client 2 selects SubstructureNotify on root
root2 = d2.screen().root
root2.change_attributes(event_mask=Xlib.X.SubstructureNotifyMask)
d2.sync()
# Client 1 creates and maps a window
w = root.create_window(0, 0, 100, 100, 0, d1.screen().root_depth)
w.map()
d1.sync()
time.sleep(0.5)
# Client 2 should receive CreateNotify + MapNotify
got_create = False
got_map = False
while d2.pending_events():
    ev = d2.next_event()
    if ev.type == Xlib.X.CreateNotify:
        got_create = True
    elif ev.type == Xlib.X.MapNotify:
        got_map = True
# Clean up
w.destroy()
d1.sync()
d1.close()
d2.close()
results = []
if got_create: results.append("CreateNotify")
if got_map: results.append("MapNotify")
if len(results) == 2:
    print(f"PASS: received {results}")
else:
    print(f"FAIL: only received {results}")
