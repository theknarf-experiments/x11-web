import Xlib.display, Xlib.Xatom, sys
d = Xlib.display.Display()
root = d.screen().root
check_atom = d.intern_atom("_NET_SUPPORTING_WM_CHECK")
name_atom = d.intern_atom("_NET_WM_NAME")
utf8 = d.intern_atom("UTF8_STRING")
# Root must have _NET_SUPPORTING_WM_CHECK pointing to a child window
prop = root.get_full_property(check_atom, Xlib.Xatom.WINDOW)
assert prop is not None, "root missing _NET_SUPPORTING_WM_CHECK"
check_win_id = prop.value[0]
# That child window must also point to itself
check_win = d.create_resource_object("window", check_win_id)
prop2 = check_win.get_full_property(check_atom, Xlib.Xatom.WINDOW)
assert prop2 is not None, "check window missing self-reference"
assert prop2.value[0] == check_win_id, "self-reference mismatch"
# Check window must have _NET_WM_NAME
name_prop = check_win.get_full_property(name_atom, utf8)
assert name_prop is not None, "check window missing _NET_WM_NAME"
d.close()
print("wm-check-ok")
