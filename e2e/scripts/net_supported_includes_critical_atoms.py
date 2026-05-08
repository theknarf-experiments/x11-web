from Xlib import display, X, Xatom
d = display.Display()
root = d.screen().root
net_supported = d.intern_atom('_NET_SUPPORTED')
prop = root.get_full_property(net_supported, Xatom.ATOM)
if prop and prop.value:
    atoms = list(prop.value)
    wm_name = d.intern_atom('_NET_WM_NAME')
    wm_state = d.intern_atom('_NET_WM_STATE')
    client_list = d.intern_atom('_NET_CLIENT_LIST')
    print(f"has_wm_name={wm_name in atoms}")
    print(f"has_wm_state={wm_state in atoms}")
    print(f"has_client_list={client_list in atoms}")
    print(f"atom_count={len(atoms)}")
else:
    print("no_net_supported=True")
d.close()
