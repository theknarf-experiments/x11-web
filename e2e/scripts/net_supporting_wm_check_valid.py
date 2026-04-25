import Xlib.display, Xlib.Xatom
d = Xlib.display.Display(":99")
root = d.screen().root
wm_check = d.intern_atom("_NET_SUPPORTING_WM_CHECK")
net_wm_name = d.intern_atom("_NET_WM_NAME")
utf8 = d.intern_atom("UTF8_STRING")
# Get WM check window from root
prop = root.get_property(wm_check, Xlib.Xatom.WINDOW, 0, 1)
assert prop is not None, "No _NET_SUPPORTING_WM_CHECK on root"
check_wid = prop.value[0]
print(f"WM_CHECK_WINDOW={check_wid:#x}")
# The check window should also have _NET_SUPPORTING_WM_CHECK pointing to itself
check_win = d.create_resource_object("window", check_wid)
prop2 = check_win.get_property(wm_check, Xlib.Xatom.WINDOW, 0, 1)
assert prop2 is not None, "Check window missing self-reference"
assert prop2.value[0] == check_wid, "Self-reference mismatch"
# Check window should have _NET_WM_NAME
name_prop = check_win.get_property(net_wm_name, utf8, 0, 100)
if name_prop:
    name = bytes(name_prop.value).decode("utf-8")
    print(f"WM_NAME={name}")
print("WM_CHECK_OK")
d.close()
