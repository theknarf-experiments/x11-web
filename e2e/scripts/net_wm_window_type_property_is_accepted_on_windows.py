import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create a window
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth)

# Set _NET_WM_WINDOW_TYPE to DOCK
type_atom = d.intern_atom('_NET_WM_WINDOW_TYPE')
dock_atom = d.intern_atom('_NET_WM_WINDOW_TYPE_DOCK')
w.change_property(type_atom, d.intern_atom('ATOM'), 32, [dock_atom])
d.sync()

# Read it back
prop = w.get_full_property(type_atom, d.intern_atom('ATOM'))
if prop and len(prop.value) > 0:
    print(f"result=OK,type={prop.value[0]}")
else:
    print("result=FAIL")

w.destroy()
d.close()
