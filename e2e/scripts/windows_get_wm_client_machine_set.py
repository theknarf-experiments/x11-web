import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
                        Xlib.X.InputOutput, Xlib.X.CopyFromParent)
machine_atom = d.intern_atom('WM_CLIENT_MACHINE')
prop = w.get_full_property(machine_atom, Xlib.X.AnyPropertyType)
if prop and prop.value:
    hostname = bytes(prop.value).decode('utf-8', errors='replace')
    print(f"machine={hostname}")
    print(f"machine_set=true")
else:
    print("machine_set=false")
w.destroy()
d.close()
