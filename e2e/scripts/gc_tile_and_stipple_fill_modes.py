import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# Create a 2x2 tile pixmap
tile = screen.root.create_pixmap(2, 2, screen.root_depth)
tile_gc = tile.create_gc(foreground=0xFF0000)
tile.fill_rectangle(tile_gc, 0, 0, 1, 1)
tile_gc2 = tile.create_gc(foreground=0x00FF00)
tile.fill_rectangle(tile_gc2, 1, 0, 1, 1)
tile.fill_rectangle(tile_gc2, 0, 1, 1, 1)
tile_gc3 = tile.create_gc(foreground=0x0000FF)
tile.fill_rectangle(tile_gc3, 1, 1, 1, 1)
d.sync()

# Create GC with tiled fill
gc = w.create_gc(fill_style=Xlib.X.FillTiled, tile=tile)
w.fill_rectangle(gc, 0, 0, 100, 100)
d.sync()

# Create a 2x2 stipple bitmap
stipple = screen.root.create_pixmap(2, 2, 1)
stip_gc = stipple.create_gc(foreground=1, background=0)
import struct
# Checkerboard pattern
stipple.put_image(stip_gc, 0, 0, 2, 2, Xlib.X.XYBitmap, 1, 0,
    struct.pack('BB', 0b01, 0b10) + b'\\x00\\x00')
d.sync()

# Create GC with stippled fill
gc2 = w.create_gc(
    fill_style=Xlib.X.FillOpaqueStippled,
    stipple=stipple,
    foreground=0xFFFF00,
    background=0x000000,
)
w.fill_rectangle(gc2, 0, 0, 50, 50)
d.sync()

print("tile_stipple=OK")

tile.free()
stipple.free()
gc.free()
gc2.free()
w.destroy()
d.close()
