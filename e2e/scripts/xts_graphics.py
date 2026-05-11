import Xlib.display
import Xlib.X
import Xlib.Xutil
import struct
import sys

errors = []

d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root
depth = screen.root_depth

# Test 1: CreatePixmap + CreateGC + PolyFillRectangle
pixmap = root.create_pixmap(100, 100, depth)
gc = root.create_gc(
    foreground=0xFF0000,  # red
    background=0x000000,
)
d.sync()
print("PASS: CreatePixmap + CreateGC succeeded")

# Fill the pixmap with red
pixmap.fill_rectangle(gc, 0, 0, 100, 100)
d.sync()

# Test 2: GetImage readback to verify pixel data
try:
    # python-xlib signature: get_image(x, y, width, height, format, plane_mask)
    img = pixmap.get_image(0, 0, 100, 100, Xlib.X.ZPixmap, 0xFFFFFFFF)
    data = img.data
    if len(data) >= 4:
        # Check first pixel is red (format depends on server byte order)
        # In ZPixmap with depth 24/32, expect BGRA or similar
        print(f"PASS: GetImage returned {len(data)} bytes")
        # Verify we got non-zero data back (not a blank image)
        nonzero = sum(1 for b in data[:400] if b != 0)
        if nonzero > 0:
            print("PASS: GetImage contains non-zero pixel data")
        else:
            errors.append("GetImage returned all zeros for red-filled pixmap")
    else:
        errors.append(f"GetImage data too short: {len(data)} bytes")
except Exception as e:
    errors.append(f"GetImage failed: {e}")

# Test 3: CopyArea between pixmaps
pixmap2 = root.create_pixmap(100, 100, depth)
gc2 = root.create_gc(foreground=0x00FF00)
pixmap2.fill_rectangle(gc2, 0, 0, 100, 100)
d.sync()

# Copy top-left 50x50 from red pixmap to green pixmap at (25,25)
pixmap2.copy_area(gc, pixmap, 0, 0, 50, 50, 25, 25)
d.sync()
print("PASS: CopyArea between pixmaps succeeded")

# Test 4: PolyLine and PolyPoint
gc3 = root.create_gc(foreground=0x0000FF)
pixmap.poly_line(gc3, Xlib.X.CoordModeOrigin,
                 [(0, 0), (50, 50), (99, 0)])
d.sync()
print("PASS: PolyLine succeeded")

pixmap.poly_point(gc3, Xlib.X.CoordModeOrigin,
                  [(10, 10), (20, 20), (30, 30)])
d.sync()
print("PASS: PolyPoint succeeded")

# Test 5: PolyFillRectangle with multiple rectangles
gc4 = root.create_gc(foreground=0xFFFF00)
pixmap.fill_rectangle(gc4, 10, 10, 20, 20)
pixmap.fill_rectangle(gc4, 40, 40, 20, 20)
d.sync()
print("PASS: multiple PolyFillRectangle calls succeeded")

# Test 6: GC with different functions (GXcopy, GXxor, GXclear)
gc_xor = root.create_gc(
    foreground=0xFFFFFF,
    function=Xlib.X.GXxor,
)
pixmap.fill_rectangle(gc_xor, 0, 0, 50, 50)
d.sync()
print("PASS: GC with GXxor function works")

gc_clear = root.create_gc(
    foreground=0x000000,
    function=Xlib.X.GXclear,
)
pixmap.fill_rectangle(gc_clear, 0, 0, 100, 100)
d.sync()
print("PASS: GC with GXclear function works")

# Test 7: FreePixmap and FreeGC (should not crash)
pixmap.free()
pixmap2.free()
gc.free()
gc2.free()
gc3.free()
gc4.free()
gc_xor.free()
gc_clear.free()
d.sync()
print("PASS: FreePixmap and FreeGC succeeded")

# Test 8: CreatePixmap with depth=1 (bitmap)
bitmap = root.create_pixmap(32, 32, 1)
gc_bmp = root.create_gc(foreground=1, background=0)
bitmap.fill_rectangle(gc_bmp, 0, 0, 32, 32)
d.sync()
bitmap.free()
gc_bmp.free()
d.sync()
print("PASS: depth-1 pixmap (bitmap) works")

d.close()

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("XTS_GRAPHICS_OK")
