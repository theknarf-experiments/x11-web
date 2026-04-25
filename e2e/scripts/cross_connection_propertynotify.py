import Xlib.display, Xlib.X, Xlib.Xatom, time, threading
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()
root = d1.screen().root
w = root.create_window(0, 0, 100, 100, 0, d1.screen().root_depth)
w.map()
d1.sync()
# Client 2 selects PropertyChangeMask on the window
w2 = d2.create_resource_object("window", w.id)
w2.change_attributes(event_mask=Xlib.X.PropertyChangeMask)
d2.sync()
# Client 1 changes a property on the window
test_atom = d1.intern_atom("TEST_CROSS_PROP")
w.change_property(test_atom, Xlib.Xatom.STRING, 8, b"hello")
d1.sync()
time.sleep(0.5)
# Client 2 should receive PropertyNotify
got_notify = False
while d2.pending_events():
    ev = d2.next_event()
    if ev.type == Xlib.X.PropertyNotify:
        got_notify = True
        break
d1.close()
d2.close()
if got_notify:
    print("PASS: cross-connection PropertyNotify delivered")
else:
    print("FAIL: no PropertyNotify received")
