import Xlib.display, Xlib.X, Xlib.protocol.event
import struct
d = Xlib.display.Display()
screen = d.screen()
root = screen.root

w = root.create_window(0, 0, 200, 200, 0, screen.root_depth,
                        Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

net_wm_state = d.intern_atom('_NET_WM_STATE')
fullscreen = d.intern_atom('_NET_WM_STATE_FULLSCREEN')

# Send ClientMessage to root to request fullscreen
ev = Xlib.protocol.event.ClientMessage(
    window=w, client_type=net_wm_state, data=(32, [1, fullscreen, 0, 1, 0]))
root.send_event(ev, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()
import time
time.sleep(0.1)

# Check if fullscreen state was set
prop = w.get_full_property(net_wm_state, Xlib.X.AnyPropertyType)
if prop and prop.value is not None:
    atoms = list(prop.value)
    print(f"has_fullscreen={fullscreen in atoms}")
else:
    print("has_fullscreen=false")

print("state_change_test=ok")
w.destroy()
d.close()
