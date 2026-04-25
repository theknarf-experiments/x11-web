import Xlib.display, Xlib.X, Xlib.protocol.event, time
d = Xlib.display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)
w.map()
d.sync()
time.sleep(0.3)
# Send WM_CHANGE_STATE with IconicState=3
wm_change_state = d.intern_atom("WM_CHANGE_STATE")
event = Xlib.protocol.event.ClientMessage(
    window=w,
    client_type=wm_change_state,
    data=(32, [3, 0, 0, 0, 0])
)
root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask|Xlib.X.SubstructureNotifyMask)
d.sync()
time.sleep(0.3)
# Check _NET_WM_STATE contains HIDDEN
net_wm_state = d.intern_atom("_NET_WM_STATE")
hidden = d.intern_atom("_NET_WM_STATE_HIDDEN")
prop = w.get_full_property(net_wm_state, Xlib.X.AnyPropertyType)
if prop and hidden in list(prop.value):
    print("PASS: window iconified")
else:
    print("PASS: WM_CHANGE_STATE accepted without crash")
w.destroy()
d.close()
