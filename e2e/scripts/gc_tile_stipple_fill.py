from Xlib import X, display, Xutil
d = display.Display(":99")
root = d.screen().root
# Create a test window
w = root.create_window(0, 0, 200, 200, 0, d.screen().root_depth)
w.map()
d.sync()
# Create a tile pixmap (2x2 checkerboard)
tile = w.create_pixmap(2, 2, d.screen().root_depth)
gc_tile = tile.create_gc(foreground=0xFF0000)
tile.fill_rectangle(gc_tile, 0, 0, 1, 1)
tile.fill_rectangle(gc_tile, 1, 1, 1, 1)
# Create GC with tiled fill_style
gc = w.create_gc(foreground=0xFF0000, fill_style=X.FillTiled, tile=tile)
# FillRectangle with tile pattern
w.fill_rectangle(gc, 10, 10, 50, 50)
d.sync()
# Create stipple pixmap (1-bit, 2x2 pattern)
stipple = w.create_pixmap(2, 2, 1)
gc_stip = stipple.create_gc(foreground=1)
stipple.fill_rectangle(gc_stip, 0, 0, 1, 1)
stipple.fill_rectangle(gc_stip, 1, 1, 1, 1)
# Create GC with stippled fill_style
gc2 = w.create_gc(foreground=0x00FF00, fill_style=X.FillStippled, stipple=stipple)
w.fill_rectangle(gc2, 70, 10, 50, 50)
d.sync()
w.destroy()
d.close()
print("gc-fill-test-pass")
