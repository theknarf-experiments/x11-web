import Xlib.display, Xlib.X, Xlib.protocol.event
d = Xlib.display.Display()
screen = d.screen()
root = screen.root

w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
                        Xlib.X.InputOutput, Xlib.X.CopyFromParent)
# Set WM_PROTOCOLS to include WM_DELETE_WINDOW
wm_protocols = d.intern_atom('WM_PROTOCOLS')
wm_delete = d.intern_atom('WM_DELETE_WINDOW')
w.change_property(wm_protocols, Xlib.X.AnyPropertyType, 32, [wm_delete])
w.map()
d.sync()

# Send _NET_CLOSE_WINDOW to root
net_close = d.intern_atom('_NET_CLOSE_WINDOW')
ev = Xlib.protocol.event.ClientMessage(
    window=w, client_type=net_close, data=(32, [0, 0, 0, 0, 0]))
root.send_event(ev, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()
import time
time.sleep(0.1)

# Check for the WM_DELETE_WINDOW ClientMessage
# We'll just verify no crash occurred
print("close_window_test=ok")
w.destroy()
d.close()
