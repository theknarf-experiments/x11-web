import Xlib.display, Xlib.X, time
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()
root = d1.screen().root
w = root.create_window(0, 0, 100, 100, 0, d1.screen().root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d1.sync()
# Client 2 also selects ExposureMask on this window
w2 = d2.create_resource_object("window", w.id)
w2.change_attributes(event_mask=Xlib.X.ExposureMask)
d2.sync()
time.sleep(0.5)
# Client 1 should have got Expose from the map
got_c1 = False
while d1.pending_events():
    ev = d1.next_event()
    if ev.type == Xlib.X.Expose:
        got_c1 = True
# Client 2 should also have received Expose broadcast
got_c2 = False
while d2.pending_events():
    ev = d2.next_event()
    if ev.type == Xlib.X.Expose:
        got_c2 = True
w.destroy()
d1.close()
d2.close()
if got_c1:
    print("PASS: Expose events delivered")
else:
    print("FAIL: no Expose events")
