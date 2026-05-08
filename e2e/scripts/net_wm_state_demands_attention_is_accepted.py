import Xlib.display, Xlib.X, Xlib.protocol.event
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)
w.map()
d.sync()

# Set DEMANDS_ATTENTION state
state_atom = d.intern_atom('_NET_WM_STATE')
attention_atom = d.intern_atom('_NET_WM_STATE_DEMANDS_ATTENTION')
event = Xlib.protocol.event.ClientMessage(
    window=w,
    client_type=state_atom,
    data=(32, [1, attention_atom, 0, 0, 0])
)
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()

import time
time.sleep(0.1)

# Verify the state was recorded
prop = w.get_full_property(state_atom, d.intern_atom('ATOM'))
if prop and attention_atom in prop.value:
    print("demands_attention_set=True")
else:
    print("demands_attention_set=False")

w.destroy()
d.close()
