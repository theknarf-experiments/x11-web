import Xlib.display, Xlib.X, Xlib.Xatom
import struct

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set _NET_WM_STATE with multiple state atoms
net_wm_state = d.intern_atom('_NET_WM_STATE')
above = d.intern_atom('_NET_WM_STATE_ABOVE')
focused = d.intern_atom('_NET_WM_STATE_FOCUSED')

w.change_property(net_wm_state, Xlib.Xatom.ATOM, 32, [above, focused])
d.sync()

# Read it back
prop = w.get_full_property(net_wm_state, Xlib.Xatom.ATOM)
if prop and len(prop.value) > 0:
    raw = bytes(prop.value) if isinstance(prop.value, (bytes, bytearray)) else b''
    if raw:
        atoms = struct.unpack('<' + 'I' * (len(raw) // 4), raw[:len(raw) - len(raw) % 4])
    else:
        atoms = list(prop.value)
    if above in atoms and focused in atoms:
        print("NET_WM_STATE_OK")

d.close()
