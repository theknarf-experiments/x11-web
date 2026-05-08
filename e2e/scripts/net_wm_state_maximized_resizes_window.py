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
max_vert = d.intern_atom('_NET_WM_STATE_MAXIMIZED_VERT')
max_horz = d.intern_atom('_NET_WM_STATE_MAXIMIZED_HORZ')

event = Xlib.protocol.event.ClientMessage(
    window=w, client_type=net_wm_state,
    data=(32, [1, max_vert, max_horz, 1, 0]))
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()
time.sleep(1)

geom = w.get_geometry()
print(f"maximize_ok={geom.width == screen.width_in_pixels and geom.height == screen.height_in_pixels}")

w.destroy()
d.close()
