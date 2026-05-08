import Xlib.display, Xlib.X
import struct

d = Xlib.display.Display()
screen = d.screen()

# Create parent and child
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
parent.map()
d.sync()

child = parent.create_window(10, 10, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
child.map()
d.sync()

# Check WM_STATE on child (format=32 → array.array of CARDINALs, len in elements)
import time; time.sleep(0.2)
wm_state_atom = d.intern_atom("WM_STATE")
prop = child.get_full_property(wm_state_atom, wm_state_atom)
if prop and len(prop.value) >= 1:
    state_val = int(prop.value[0])
    if state_val == 1:  # NormalState
        print("CHILD_WM_STATE_OK")
    else:
        print(f"CHILD_WM_STATE={state_val}")
else:
    print("NO_CHILD_WM_STATE")

d.close()
