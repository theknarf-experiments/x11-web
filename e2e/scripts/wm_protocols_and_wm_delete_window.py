import Xlib.display, Xlib.X, Xlib.Xatom

d = Xlib.display.Display()
screen = d.screen()

# Create a window with WM_DELETE_WINDOW protocol
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set WM_PROTOCOLS
wm_protocols = d.intern_atom('WM_PROTOCOLS')
wm_delete = d.intern_atom('WM_DELETE_WINDOW')

import struct
w.change_property(wm_protocols, Xlib.Xatom.ATOM, 32,
    [wm_delete])
d.sync()

# Read it back
prop = w.get_full_property(wm_protocols, Xlib.Xatom.ATOM)
if prop and len(prop.value) > 0:
    # prop.value is an array of ints in python-xlib (one per atom),
    # or raw bytes in some older versions — handle both
    raw = bytes(prop.value) if isinstance(prop.value, (bytes, bytearray)) else b''
    if raw:
        atoms = struct.unpack('<' + 'I' * (len(raw) // 4), raw[:len(raw) - len(raw) % 4])
    else:
        atoms = list(prop.value)
    if wm_delete in atoms:
        print("WM_DELETE_WINDOW_OK")

d.close()
