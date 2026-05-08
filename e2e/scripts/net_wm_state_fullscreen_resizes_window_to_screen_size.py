import Xlib.display, Xlib.X, Xlib.protocol.event, time

d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(50, 50, 400, 300, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.PropertyChangeMask)
w.map()
d.sync()
time.sleep(0.5)

net_wm_state = d.intern_atom('_NET_WM_STATE')
fullscreen_atom = d.intern_atom('_NET_WM_STATE_FULLSCREEN')

event = Xlib.protocol.event.ClientMessage(
    window=w, client_type=net_wm_state,
    data=(32, [1, fullscreen_atom, 0, 1, 0]))
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()
time.sleep(1)

geom = w.get_geometry()
sw = screen.width_in_pixels
sh = screen.height_in_pixels
print(f"fullscreen_ok={geom.width == sw and geom.height == sh}")

# Remove fullscreen
event2 = Xlib.protocol.event.ClientMessage(
    window=w, client_type=net_wm_state,
    data=(32, [0, fullscreen_atom, 0, 1, 0]))
screen.root.send_event(event2, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()
time.sleep(1)

geom2 = w.get_geometry()
print(f"restore_ok={geom2.width == 400 and geom2.height == 300}")

w.destroy()
d.close()
