import Xlib.display
d = Xlib.display.Display(":99")
root = d.screen().root
net_sup = d.intern_atom("_NET_SUPPORTED")
prop = root.get_property(net_sup, 0, 0, 1000)
assert prop is not None, "No _NET_SUPPORTED"
atoms = list(prop.value)
# Check for critical EWMH atoms
required = [
    "_NET_WM_NAME", "_NET_WM_STATE", "_NET_ACTIVE_WINDOW",
    "_NET_SUPPORTING_WM_CHECK", "_NET_WM_STATE_FULLSCREEN",
    "_NET_CLIENT_LIST", "_NET_WM_WINDOW_TYPE",
]
for name in required:
    atom_id = d.intern_atom(name)
    assert atom_id in atoms, f"{name} (atom {atom_id}) not in _NET_SUPPORTED"
print(f"SUPPORTED_COUNT={len(atoms)}")
print("EWMH_OK")
d.close()
