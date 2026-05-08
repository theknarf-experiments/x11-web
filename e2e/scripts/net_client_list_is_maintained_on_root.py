import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create and map a window
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth)
w.map()
d.sync()

import time
time.sleep(0.1)

# Check _NET_CLIENT_LIST contains our window
cl_atom = d.intern_atom('_NET_CLIENT_LIST')
prop = screen.root.get_full_property(cl_atom, d.intern_atom('WINDOW'))
if prop and w.id in prop.value:
    print("result=OK")
else:
    print(f"result=NOT_FOUND,wid={w.id:#x},list={list(prop.value) if prop else 'None'}")

w.destroy()
d.close()
