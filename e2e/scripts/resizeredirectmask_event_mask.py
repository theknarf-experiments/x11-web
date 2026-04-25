import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root
# ResizeRedirectMask = 0x40000
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    event_mask=0x40000)
d.sync()
w.destroy()
d.sync()
d.close()
print("PASS: ResizeRedirectMask accepted")
