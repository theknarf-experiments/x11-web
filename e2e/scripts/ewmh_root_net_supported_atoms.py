import Xlib.display, Xlib.X, sys
d = Xlib.display.Display()
root = d.screen().root
net_supported = d.intern_atom("_NET_SUPPORTED")
prop = root.get_full_property(net_supported, Xlib.Xatom.ATOM)
if prop is None:
    print("ewmh-fail: _NET_SUPPORTED missing")
    sys.exit(1)
atoms = list(prop.value)
# Check required EWMH atoms
required = ["_NET_WM_NAME", "_NET_WM_STATE", "_NET_ACTIVE_WINDOW",
    "_NET_WM_WINDOW_TYPE", "_NET_SUPPORTING_WM_CHECK", "_NET_CLIENT_LIST"]
missing = []
for name in required:
    a = d.intern_atom(name, True)
    if a == 0 or a not in atoms:
        missing.append(name)
d.close()
if missing:
    print(f"ewmh-missing: {missing}")
    sys.exit(1)
print(f"ewmh-ok: {len(atoms)} supported atoms")
