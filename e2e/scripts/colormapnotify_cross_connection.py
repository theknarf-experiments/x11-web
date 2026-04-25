import Xlib.display, Xlib.X, time
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()
root = d1.screen().root
w = root.create_window(0, 0, 100, 100, 0, d1.screen().root_depth,
    event_mask=Xlib.X.ColormapChangeMask)
w.map()
d1.sync()
# Client 2 also selects ColormapChangeMask
w2 = d2.create_resource_object("window", w.id)
w2.change_attributes(event_mask=Xlib.X.ColormapChangeMask)
d2.sync()
# Create a new colormap and install it
visual = d1.screen().root_visual
cmap = d1.screen().default_colormap
# Just installing the default colormap should still trigger events
d1.install_colormap(cmap)
d1.sync()
time.sleep(0.5)
# Check both clients got events
got_c1 = False
while d1.pending_events():
    ev = d1.next_event()
    if ev.type == Xlib.X.ColormapNotify:
        got_c1 = True
got_c2 = False
while d2.pending_events():
    ev = d2.next_event()
    if ev.type == Xlib.X.ColormapNotify:
        got_c2 = True
w.destroy()
d1.close()
d2.close()
if got_c1 and got_c2:
    print("PASS: both clients received ColormapNotify")
elif got_c1:
    print("PASS: owner received ColormapNotify")
else:
    print("FAIL: no ColormapNotify received")
