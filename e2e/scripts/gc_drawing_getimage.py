import Xlib.display, Xlib.X
d = Xlib.display.Display(":99")
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    background_pixel=screen.black_pixel)
w.map()
d.sync()
# Create GC with foreground color
gc = w.create_gc(foreground=0xFF0000, background=0)
# Draw a rectangle
w.fill_rectangle(gc, 10, 10, 80, 80)
d.sync()
# Create pixmap and draw into it
pm = w.create_pixmap(50, 50, screen.root_depth)
gc2 = pm.create_gc(foreground=0x00FF00)
pm.fill_rectangle(gc2, 0, 0, 50, 50)
d.sync()
# GetImage from pixmap (ZPixmap format)
img = pm.get_image(0, 0, 50, 50, Xlib.X.ZPixmap, 0xFFFFFFFF)
assert img is not None, "GetImage returned None"
data = bytes(img.data)
assert len(data) > 0, "GetImage returned empty data"
# Verify green pixel (BGRA in ZPixmap at depth 24/32)
# The first pixel should be green: B=0, G=FF, R=0
found_green = False
for i in range(0, min(len(data), 16), 4):
    b, g, r = data[i], data[i+1], data[i+2]
    if g > 200 and r < 50 and b < 50:
        found_green = True
        break
assert found_green, f"Expected green pixel, got bytes: {data[:16].hex()}"
pm.free()
gc.free()
gc2.free()
w.destroy()
d.sync()
print("DRAWING_OPS_OK")
d.close()
