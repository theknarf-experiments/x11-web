import Xlib.display, Xlib.X, Xlib.protocol.event
import time
d = Xlib.display.Display()
screen = d.screen()

# Create parent and dialog windows
parent = screen.root.create_window(0, 0, 400, 300, 0, screen.root_depth)
parent.map()
d.sync()

dialog = screen.root.create_window(50, 50, 200, 150, 0, screen.root_depth)
dialog.map()
d.sync()
time.sleep(0.1)

# Set MODAL state via ClientMessage
state_atom = d.intern_atom('_NET_WM_STATE')
modal_atom = d.intern_atom('_NET_WM_STATE_MODAL')
event = Xlib.protocol.event.ClientMessage(
    window=dialog,
    client_type=state_atom,
    data=(32, [1, modal_atom, 0, 0, 0])  # action=1 (add)
)
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()
time.sleep(0.1)

# Verify MODAL state was set
prop = dialog.get_full_property(state_atom, d.intern_atom('ATOM'))
if prop and modal_atom in prop.value:
    print("modal_set=True")
else:
    print("modal_set=False")

# Verify dialog is above parent in stacking order
tree = screen.root.query_tree()
children = [c.id for c in tree.children]
if parent.id in children and dialog.id in children:
    p_idx = children.index(parent.id)
    d_idx = children.index(dialog.id)
    print(f"dialog_above_parent={d_idx > p_idx}")

dialog.destroy()
parent.destroy()
d.close()
