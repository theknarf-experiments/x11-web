import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
                        Xlib.X.InputOutput, Xlib.X.CopyFromParent)
frame_atom = d.intern_atom('_NET_FRAME_EXTENTS')
prop = w.get_full_property(frame_atom, Xlib.X.AnyPropertyType)
if prop and prop.value is not None:
    extents = list(prop.value)
    print(f"frame_extents={extents}")
    print(f"frame_set=true")
else:
    print("frame_set=false")
w.destroy()
d.close()
