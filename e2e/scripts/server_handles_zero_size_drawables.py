import Xlib.display, Xlib.X, sys
d = Xlib.display.Display()
root = d.screen().root
# Create window with minimum size
w = root.create_window(0, 0, 1, 1, 0, d.screen().root_depth)
w.map()
d.sync()
# Draw operations on tiny window
gc = w.create_gc()
w.poly_point(gc, Xlib.X.CoordModeOrigin, [(0, 0)])
w.poly_fill_rectangle(gc, [(0, 0, 1, 1)])
w.clear_area(0, 0, 1, 1)
d.sync()
gc.free()
w.destroy()
d.close()
print("zero-size-ok")
