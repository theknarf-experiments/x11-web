import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

gc = w.create_gc(foreground=0xFF0000, line_width=2)
# Draw a full circle (arc from 0 to 360 degrees, in 64ths of a degree)
w.arc(gc, 50, 50, 100, 100, 0, 360 * 64)
d.sync()

# Fill arc (pie slice)
gc_fill = w.create_gc(foreground=0x00FF00, arc_mode=Xlib.X.ArcPieSlice)
w.fill_arc(gc_fill, 50, 50, 100, 100, 0, 90 * 64)
d.sync()

print("arcs_drawn=ok")

w.destroy()
d.close()
