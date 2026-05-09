import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create a pixmap and draw to it
pm = screen.root.create_pixmap(100, 100, screen.root_depth)
gc = pm.create_gc(foreground=0xFF0000)
pm.fill_rectangle(gc, 0, 0, 100, 100)
d.sync()

# Create a window and copy from pixmap
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

w.copy_area(gc, pm, 0, 0, 100, 100, 0, 0)
d.sync()

# GetImage from window to verify. python-xlib's signature is
# (x, y, width, height, format, plane_mask) — note format and plane_mask
# come in that order, opposite of the X11 wire-level GetImage request.
img = w.get_image(10, 10, 1, 1, Xlib.X.ZPixmap, 0xFFFFFFFF)
data = bytes(img.data)
if len(data) >= 4:
    import struct
    pixel = struct.unpack('<I', data[:4])[0] & 0xFFFFFF
    print(f"pixel={pixel:#08x}")
    print(f"is_red={pixel == 0xFF0000}")
else:
    print(f"data_len={len(data)}")

pm.free()
gc.free()
w.destroy()
d.close()
