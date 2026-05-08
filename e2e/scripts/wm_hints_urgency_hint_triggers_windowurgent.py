from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
hints = {}
hints['flags'] = Xutil.UrgencyHint
w.set_wm_hints(hints)
d.sync()

read_hints = w.get_wm_hints()
flags = getattr(read_hints, 'flags', 0) if read_hints else 0
urgent = bool(flags & Xutil.UrgencyHint)
print(f"urgent={urgent}")
w.destroy()
d.close()
