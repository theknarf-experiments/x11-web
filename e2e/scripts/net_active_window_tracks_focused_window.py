import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create and map a window, set focus
w = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w.map()
d.sync()

import time
time.sleep(0.1)

# Check _NET_ACTIVE_WINDOW
active_atom = d.intern_atom('_NET_ACTIVE_WINDOW')
prop = screen.root.get_full_property(active_atom, d.intern_atom('WINDOW'))
# Just verify the property exists and has a value
if prop and len(prop.value) > 0:
    print(f"result=OK,active={prop.value[0]:#x}")
else:
    print("result=NO_ACTIVE")

w.destroy()
d.close()
