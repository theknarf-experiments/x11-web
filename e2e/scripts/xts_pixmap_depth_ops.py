import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
depth = d.screen().root_depth

# Test 1: Create pixmap at screen depth
try:
    pm = root.create_pixmap(64, 64, depth)
    gc = root.create_gc(foreground=0xFF0000)
    pm.fill_rectangle(gc, 0, 0, 64, 64)
    d.sync()
    passed += 1; print(f"PASS: create pixmap at depth {depth}")
except Exception as e:
    failed += 1; print(f"FAIL: pixmap at depth {depth}: {e}")

# Test 2: Create pixmap at depth 1 (bitmap)
try:
    pm1 = root.create_pixmap(32, 32, 1)
    gc1 = pm1.create_gc(foreground=1, background=0)
    pm1.fill_rectangle(gc1, 0, 0, 32, 32)
    d.sync()
    passed += 1; print("PASS: create depth-1 bitmap pixmap")
    gc1.free()
    pm1.free()
except Exception as e:
    failed += 1; print(f"FAIL: depth-1 pixmap: {e}")

# Test 3: GetImage from pixmap
try:
    # python-xlib signature: get_image(x, y, w, h, format, plane_mask)
    img = pm.get_image(0, 0, 64, 64, Xlib.X.ZPixmap, 0xFFFFFFFF)
    if img and len(img.data) > 0:
        passed += 1; print(f"PASS: GetImage returned {len(img.data)} bytes")
    else:
        failed += 1; print("FAIL: GetImage returned empty data")
except Exception as e:
    failed += 1; print(f"FAIL: GetImage: {e}")

# Test 4: CopyArea between pixmaps
try:
    pm2 = root.create_pixmap(64, 64, depth)
    gc2 = root.create_gc()
    pm2.copy_area(gc2, pm, 0, 0, 64, 64, 0, 0)
    d.sync()
    passed += 1; print("PASS: CopyArea between pixmaps")
    gc2.free()
    pm2.free()
except Exception as e:
    failed += 1; print(f"FAIL: CopyArea: {e}")

gc.free()
pm.free()
d.close()
print(f"xts-pixmap-depth: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
