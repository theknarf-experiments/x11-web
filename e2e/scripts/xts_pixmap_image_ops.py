import Xlib.display, Xlib.X, Xlib.Xutil, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()
depth = screen.root_depth

# Test 1: CreatePixmap
try:
    pm = root.create_pixmap(100, 100, depth)
    if pm.id > 0:
        passed += 1; print(f"PASS: CreatePixmap id=0x{pm.id:x}")
    else:
        failed += 1; print("FAIL: CreatePixmap returned 0")
except Exception as e:
    failed += 1; print(f"FAIL: CreatePixmap: {e}"); sys.exit(1)

# Test 2: Draw on pixmap
try:
    gc = pm.create_gc(foreground=screen.white_pixel)
    pm.fill_rectangle(gc, 0, 0, 100, 100)
    d.sync()
    passed += 1; print("PASS: draw on pixmap")
except Exception as e:
    failed += 1; print(f"FAIL: draw on pixmap: {e}")

# Test 3: GetImage from pixmap
try:
    image = pm.get_image(0, 0, 100, 100, 0xFFFFFFFF, Xlib.X.ZPixmap)
    if len(image.data) > 0:
        passed += 1; print(f"PASS: GetImage {len(image.data)} bytes")
    else:
        failed += 1; print("FAIL: GetImage returned empty data")
except Exception as e:
    failed += 1; print(f"FAIL: GetImage: {e}")

# Test 4: CopyArea pixmap to window
try:
    w = root.create_window(0, 0, 100, 100, 0, depth,
        Xlib.X.InputOutput, Xlib.X.CopyFromParent,
        background_pixel=screen.black_pixel)
    w.map()
    d.sync()
    gc2 = w.create_gc()
    w.copy_area(gc2, pm, 0, 0, 100, 100, 0, 0)
    d.sync()
    passed += 1; print("PASS: CopyArea pixmap->window")
except Exception as e:
    failed += 1; print(f"FAIL: CopyArea: {e}")

# Test 5: FreePixmap
try:
    pm.free()
    d.sync()
    passed += 1; print("PASS: FreePixmap")
except Exception as e:
    failed += 1; print(f"FAIL: FreePixmap: {e}")

# Test 6: PutImage (create small pixmap and put data)
try:
    pm2 = root.create_pixmap(8, 8, depth)
    gc3 = pm2.create_gc()
    # Create a small 8x8 image (all white)
    bpp = depth // 8 if depth >= 8 else 1
    data = bytes([0xFF] * (8 * 8 * bpp))
    pm2.put_image(gc3, 0, 0, 8, 8, Xlib.X.ZPixmap, depth, 0, data)
    d.sync()
    passed += 1; print("PASS: PutImage")
    pm2.free()
except Exception as e:
    failed += 1; print(f"FAIL: PutImage: {e}")

gc.free()
gc2.free()
w.destroy()
d.close()
print(f"xts-pixmap: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
