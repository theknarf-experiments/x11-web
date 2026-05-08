import Xlib.display
d = Xlib.display.Display()
# Check XIM_SERVERS atom
xim_atom = d.intern_atom('XIM_SERVERS', True)
if xim_atom:
    root = d.screen().root
    prop = root.get_full_property(xim_atom, d.intern_atom('ATOM'))
    if prop and len(prop.value) > 0:
        print(f"xim_servers_count={len(prop.value)}")
        print("xim_server_found=True")
    else:
        print("xim_server_found=False")
else:
    print("xim_atom_missing")
d.close()
