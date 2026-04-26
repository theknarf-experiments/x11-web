from Xlib import X, display, Xutil
d = display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent)
# Set WM_HINTS with urgency flag (UrgencyHint = bit 8). python-xlib
# doesn't expose Xutil.Hints; set_wm_hints accepts kwargs directly.
w.set_wm_hints(flags=Xutil.UrgencyHint)
d.sync()
# Read back and verify
got = w.get_wm_hints()
print(f'flags={got.flags if got else 0}')
w.destroy()
d.close()
