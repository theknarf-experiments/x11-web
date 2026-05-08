import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create a depth-1 pixmap
pm = screen.root.create_pixmap(10, 10, 1)
gc1 = pm.create_gc(foreground=1, background=0)
pm.fill_rectangle(gc1, 0, 0, 10, 10)
d.sync()

# Create a depth-24 window
w = screen.root.create_window(0, 0, 20, 20, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# CopyPlane from depth-1 to depth-24
gc24 = w.create_gc(foreground=0x00FF00, background=0x000000)
w.copy_plane(gc24, pm, 0, 0, 10, 10, 5, 5, 1)
d.sync()

print("copy_plane=ok")

pm.free()
w.destroy()
d.close()
