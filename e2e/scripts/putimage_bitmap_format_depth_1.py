import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 16, 2, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

gc = w.create_gc(foreground=0xFF0000, background=0x00FF00)

# Bitmap: 16 pixels wide, 2 rows. Padded to 4 bytes per row.
# Row 1: 0xAA55 = alternating bits
# Row 2: 0x55AA
import struct
bitmap = struct.pack('<HH', 0xAA55, 0) + struct.pack('<HH', 0x55AA, 0)

w.put_image(gc, 0, 0, 16, 2, Xlib.X.XYBitmap, 1, 0, bytes(bitmap))
d.sync()
print("bitmap_ok=True")

w.destroy()
d.close()
