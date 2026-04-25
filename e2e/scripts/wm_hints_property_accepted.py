from Xlib import X, display, Xutil
d = display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent)
# Set WM_HINTS with urgency flag
hints = Xutil.Hints(flags=256)  # UrgencyHint = bit 8
w.set_wm_hints(hints)
d.sync()
# Read back and verify
got = w.get_wm_hints()
print(f'flags={got.flags if got else 0}')
w.destroy()
d.close()
