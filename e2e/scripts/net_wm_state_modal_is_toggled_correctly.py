from Xlib import display, X
from Xlib.protocol import event
d = display.Display()
root = d.screen().root

w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
w.map()
d.sync()

net_wm_state = d.intern_atom('_NET_WM_STATE')
modal = d.intern_atom('_NET_WM_STATE_MODAL')

# Add MODAL
e = event.ClientMessage(window=w.id, client_type=net_wm_state, data=(32, [1, modal, 0, 1, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

# Toggle MODAL (should remove it)
e = event.ClientMessage(window=w.id, client_type=net_wm_state, data=(32, [2, modal, 0, 1, 0]))
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

import array
state_prop = w.get_full_property(net_wm_state, X.AnyPropertyType)
atoms = array.array('I', state_prop.value) if state_prop and state_prop.value else []
has_modal = modal in atoms
print(f"modal_removed={not has_modal}")

w.destroy()
d.close()
