import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask, background_pixel=0x000000)
w.map()
d.sync()

# GXcopy (3) - default
gc_copy = w.create_gc(function=Xlib.X.GXcopy, foreground=0xFF0000)
w.fill_rectangle(gc_copy, 0, 0, 50, 50)
d.sync()

# GXxor (6)
gc_xor = w.create_gc(function=Xlib.X.GXxor, foreground=0xFFFFFF)
w.fill_rectangle(gc_xor, 25, 25, 50, 50)
d.sync()

# GXinvert (10)
gc_invert = w.create_gc(function=Xlib.X.GXinvert)
w.fill_rectangle(gc_invert, 0, 0, 100, 100)
d.sync()

# Get pixel at (10, 10) - should be inverted red -> cyan.
# python-xlib signature is (x, y, w, h, format, plane_mask).
img = w.get_image(10, 10, 1, 1, Xlib.X.ZPixmap, 0xFFFFFFFF)
import struct
px = struct.unpack('<I', img.data[:4])[0] & 0xFFFFFF
# Original was 0xFF0000 (red), inverted should be 0x00FFFF (cyan)
print(f"inverted_pixel=0x{px:06x}")
print(f"gc_ops_ok=True")

gc_copy.free()
gc_xor.free()
gc_invert.free()
w.destroy()
d.close()
