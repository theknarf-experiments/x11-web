import Xlib.display, Xlib.X, Xlib.Xutil
d = Xlib.display.Display(":99")
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 200, 200, 0, screen.root_depth)
w.map()
d.sync()
# Set size hints
hints = Xlib.Xutil.WMNormalHints()
hints.flags = Xlib.Xutil.PMinSize | Xlib.Xutil.PMaxSize | Xlib.Xutil.PResizeInc
hints.min_width = 100
hints.min_height = 80
hints.max_width = 800
hints.max_height = 600
hints.width_inc = 10
hints.height_inc = 10
w.set_wm_normal_hints(hints)
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
