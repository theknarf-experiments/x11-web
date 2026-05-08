import Xlib.display, Xlib.X
import struct
d = Xlib.display.Display()
screen = d.screen()

# Read initial workarea
wa_atom = d.intern_atom('_NET_WORKAREA')
initial = screen.root.get_full_property(wa_atom, d.intern_atom('CARDINAL'))
if initial:
    vals = struct.unpack('<4I', bytes(initial.value))
    initial_w = vals[2]
else:
    initial_w = 0

# Create a dock window with a 50px left strut
dock = screen.root.create_window(0, 0, 50, screen.height_in_pixels, 0, screen.root_depth)
type_atom = d.intern_atom('_NET_WM_WINDOW_TYPE')
dock_atom = d.intern_atom('_NET_WM_WINDOW_TYPE_DOCK')
dock.change_property(type_atom, d.intern_atom('ATOM'), 32, [dock_atom])

strut_atom = d.intern_atom('_NET_WM_STRUT')
dock.change_property(strut_atom, d.intern_atom('CARDINAL'), 32, [50, 0, 0, 0])
d.sync()

# Read updated workarea
updated = screen.root.get_full_property(wa_atom, d.intern_atom('CARDINAL'))
if updated:
    vals = struct.unpack('<4I', bytes(updated.value))
    new_x = vals[0]
    new_w = vals[2]
    if new_x == 50 and new_w < initial_w:
        print("result=OK")
    else:
        print(f"result=WRONG,x={new_x},w={new_w},init_w={initial_w}")
else:
    print("result=NO_WORKAREA")

dock.destroy()
d.sync()
d.close()
