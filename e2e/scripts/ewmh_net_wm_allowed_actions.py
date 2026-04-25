import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

# Check _NET_SUPPORTED on root
net_supported = d.intern_atom("_NET_SUPPORTED")
prop = root.get_property(net_supported, Xlib.X.AnyPropertyType, 0, 1000)
if prop and len(prop.value) > 20:
    passed += 1; print(f"PASS: _NET_SUPPORTED has {len(prop.value)} atoms")
else:
    failed += 1; print(f"FAIL: _NET_SUPPORTED has {len(prop.value) if prop else 0} atoms")

# Check _NET_SUPPORTING_WM_CHECK
check_atom = d.intern_atom("_NET_SUPPORTING_WM_CHECK")
prop2 = root.get_property(check_atom, Xlib.X.AnyPropertyType, 0, 100)
if prop2 and len(prop2.value) > 0:
    check_wid = prop2.value[0]
    passed += 1; print(f"PASS: _NET_SUPPORTING_WM_CHECK = 0x{check_wid:x}")
else:
    failed += 1; print("FAIL: missing _NET_SUPPORTING_WM_CHECK")

# Check _NET_WM_NAME on root
net_wm_name = d.intern_atom("_NET_WM_NAME")
prop3 = root.get_property(net_wm_name, Xlib.X.AnyPropertyType, 0, 100)
if prop3 and b"x11-web" in bytes(prop3.value):
    passed += 1; print("PASS: _NET_WM_NAME = x11-web")
else:
    failed += 1; print(f"FAIL: _NET_WM_NAME = {prop3.value if prop3 else None}")

# Check _NET_DESKTOP_GEOMETRY
geom_atom = d.intern_atom("_NET_DESKTOP_GEOMETRY")
prop4 = root.get_property(geom_atom, Xlib.X.AnyPropertyType, 0, 100)
if prop4 and len(prop4.value) >= 2 and prop4.value[0] > 0:
    passed += 1; print(f"PASS: _NET_DESKTOP_GEOMETRY = {prop4.value[0]}x{prop4.value[1]}")
else:
    failed += 1; print(f"FAIL: _NET_DESKTOP_GEOMETRY = {prop4.value if prop4 else None}")

# Check _NET_WORKAREA
wa_atom = d.intern_atom("_NET_WORKAREA")
prop5 = root.get_property(wa_atom, Xlib.X.AnyPropertyType, 0, 100)
if prop5 and len(prop5.value) >= 4:
    passed += 1; print(f"PASS: _NET_WORKAREA = {list(prop5.value[:4])}")
else:
    failed += 1; print("FAIL: missing _NET_WORKAREA")

d.close()
print(f"ewmh_suite: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
