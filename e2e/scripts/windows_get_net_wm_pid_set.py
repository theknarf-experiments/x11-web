import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
                        Xlib.X.InputOutput, Xlib.X.CopyFromParent)
pid_atom = d.intern_atom('_NET_WM_PID')
prop = w.get_full_property(pid_atom, Xlib.X.AnyPropertyType)
if prop and prop.value:
    print(f"pid_value={prop.value[0]}")
    print(f"pid_nonzero={prop.value[0] > 0}")
else:
    print("pid_value=none")
    print("pid_nonzero=false")
w.destroy()
d.close()
