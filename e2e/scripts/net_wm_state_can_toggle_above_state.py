import Xlib.display, Xlib.X, Xlib.protocol.event
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(10, 10, 200, 200, 0, screen.root_depth)
w.map()
d.sync()

# Send _NET_WM_STATE ClientMessage to add ABOVE state
state_atom = d.intern_atom('_NET_WM_STATE')
above_atom = d.intern_atom('_NET_WM_STATE_ABOVE')

# Create ClientMessage event
event = Xlib.protocol.event.ClientMessage(
    window=w,
    client_type=state_atom,
    data=(32, [1, above_atom, 0, 0, 0])  # action=1 (add)
)
screen.root.send_event(event, event_mask=Xlib.X.SubstructureRedirectMask | Xlib.X.SubstructureNotifyMask)
d.sync()

# Check that the state was applied
prop = w.get_full_property(state_atom, d.intern_atom('ATOM'))
if prop and above_atom in prop.value:
    print("result=OK")
else:
    print(f"result=FAIL,prop={prop}")

w.destroy()
d.close()
