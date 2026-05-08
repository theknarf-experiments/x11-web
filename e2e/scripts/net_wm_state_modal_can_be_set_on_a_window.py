from Xlib import display, X
d = display.Display()
root = d.screen().root

# Create parent and modal child
parent = root.create_window(10, 10, 400, 400, 0, d.screen().root_depth,
    event_mask=X.StructureNotifyMask | X.PropertyChangeMask)
parent.map()
d.sync()

child = root.create_window(50, 50, 200, 200, 0, d.screen().root_depth)
child.map()
d.sync()

# Set WM_TRANSIENT_FOR on child
wm_transient_for = d.intern_atom('WM_TRANSIENT_FOR')
child.change_property(wm_transient_for, 33, 32, [parent.id])
d.sync()

# Set _NET_WM_STATE_MODAL on child via ClientMessage
net_wm_state = d.intern_atom('_NET_WM_STATE')
net_wm_state_modal = d.intern_atom('_NET_WM_STATE_MODAL')

# Send _NET_WM_STATE ClientMessage to root
from Xlib.protocol import event
import struct
e = event.ClientMessage(
    window=child.id,
    client_type=net_wm_state,
    data=(32, [1, net_wm_state_modal, 0, 1, 0])  # 1=_NET_WM_STATE_ADD
)
root.send_event(e, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)
d.sync()

# Read back _NET_WM_STATE to check MODAL is set
state_prop = child.get_full_property(net_wm_state, X.AnyPropertyType)
if state_prop:
    import array
    atoms = array.array('I', state_prop.value)
    has_modal = net_wm_state_modal in atoms
    print(f"has_modal={has_modal}")
else:
    print("has_modal=False")

child.destroy()
parent.destroy()
d.close()
