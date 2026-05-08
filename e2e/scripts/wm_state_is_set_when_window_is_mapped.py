import Xlib.display, Xlib.X, Xlib.Xatom
import struct

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()
import time; time.sleep(0.2)  # let the server set the property

# Check WM_STATE property. python-xlib decodes format=32 properties into
# an array.array('I', ...), so len(prop.value) is the count of CARDINALs
# (2: state + icon_window), not bytes.
wm_state_atom = d.intern_atom("WM_STATE")
prop = w.get_full_property(wm_state_atom, wm_state_atom)
if prop and len(prop.value) >= 1:
    state_val = int(prop.value[0])
    print(f"wm_state={state_val}")
    if state_val == 1:  # NormalState
        print("WM_STATE_OK")
else:
    print("NO_WM_STATE")

d.close()
