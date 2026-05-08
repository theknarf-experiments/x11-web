import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(
    0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask,
    background_pixel=screen.white_pixel,
)
w.map()
d.sync()

# Create GC and draw
gc = w.create_gc(foreground=screen.black_pixel)
w.fill_rectangle(gc, 10, 10, 30, 30)
d.sync()
print("draw_ok=True")

gc.free()
w.destroy()
d.close()
