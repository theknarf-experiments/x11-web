import Xlib.display, Xlib.X, Xlib.Xutil
d = Xlib.display.Display(":99")
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 200, 200, 0, screen.root_depth)
w.map()
d.sync()
# Set size hints. python-xlib's set_wm_normal_hints takes kwargs/dict,
# not an Xlib.Xutil.WMNormalHints instance (that class lives under
# Xlib.xobject.icccm but the kwargs interface is the supported one).
w.set_wm_normal_hints(
    flags=Xlib.Xutil.PMinSize | Xlib.Xutil.PMaxSize | Xlib.Xutil.PResizeInc,
    min_width=100, min_height=80,
    max_width=800, max_height=600,
    width_inc=10, height_inc=10,
)
d.sync()
# Read back
wm_size = d.intern_atom("WM_NORMAL_HINTS")
prop = w.get_property(wm_size, 0, 0, 100)
assert prop is not None, "WM_NORMAL_HINTS not set"
assert len(prop.value) >= 15, f"Hints too short: {len(prop.value)}"
print("WM_HINTS_OK")
w.destroy()
d.sync()
d.close()
