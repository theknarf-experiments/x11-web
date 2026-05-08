import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

gc = w.create_gc(foreground=0xFF0000, line_width=2)

# PolyLine
w.poly_line(gc, Xlib.X.CoordModeOrigin, [(0, 0), (50, 50), (100, 0)])
d.sync()

# PolySegment
w.poly_segment(gc, [(0, 100, 100, 0), (0, 0, 100, 100)])
d.sync()

# PolyRectangle
w.poly_rectangle(gc, [(10, 10, 30, 30), (50, 50, 20, 20)])
d.sync()

# PolyArc
w.poly_arc(gc, [(10, 10, 40, 40, 0, 360*64)])
d.sync()

# FillPoly
w.fill_poly(gc, Xlib.X.Convex, Xlib.X.CoordModeOrigin,
    [(50, 0), (100, 50), (50, 100), (0, 50)])
d.sync()

print("drawing_ops=ok")
w.destroy()
d.close()
