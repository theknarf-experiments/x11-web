import Xlib.display, Xlib.X, Xlib.protocol.event, time
d = Xlib.display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)
w.map()
d.sync()
time.sleep(0.3)
# Send _NET_WM_STATE toggle for fullscreen
net_wm_state = d.intern_atom("_NET_WM_STATE")
fullscreen = d.intern_atom("_NET_WM_STATE_FULLSCREEN")
# action=2 (toggle), prop1=fullscreen
event = Xlib.protocol.event.ClientMessage(
    window=w,
    client_type=net_wm_state,
    data=(32, [2, fullscreen, 0, 1, 0])
)
root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask|Xlib.X.SubstructureNotifyMask)
d.sync()
time.sleep(0.3)
# Read _NET_WM_STATE property
prop = w.get_full_property(net_wm_state, Xlib.X.AnyPropertyType)
if prop and fullscreen in list(prop.value):
    print("PASS: fullscreen state set")
else:
    val = list(prop.value) if prop else []
    print(f"FAIL: state={val}")
w.destroy()
d.close()
