import Xlib.display, Xlib.X, Xlib.Xatom, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()
w = root.create_window(0, 0, 200, 200, 0,
    screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    background_pixel=screen.black_pixel,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# Test 1: CreateGC
try:
    gc = w.create_gc(
        foreground=screen.white_pixel,
        background=screen.black_pixel,
        line_width=1)
    passed += 1; print(f"PASS: CreateGC id=0x{gc.id:x}")
except Exception as e:
    failed += 1; print(f"FAIL: CreateGC: {e}"); sys.exit(1)

# Test 2: PolyLine
try:
    w.line(gc, 10, 10, 100, 10)
    d.sync()
    passed += 1; print("PASS: PolyLine (line)")
except Exception as e:
    failed += 1; print(f"FAIL: PolyLine: {e}")

# Test 3: PolySegment
try:
    w.poly_segment(gc, [(10, 20, 100, 20), (10, 30, 100, 30)])
    d.sync()
    passed += 1; print("PASS: PolySegment")
except Exception as e:
    failed += 1; print(f"FAIL: PolySegment: {e}")

# Test 4: PolyRectangle
try:
    w.rectangle(gc, 10, 40, 80, 40)
    d.sync()
    passed += 1; print("PASS: PolyRectangle")
except Exception as e:
    failed += 1; print(f"FAIL: PolyRectangle: {e}")

# Test 5: FillPoly
try:
    w.fill_poly(gc, Xlib.X.Convex, Xlib.X.CoordModeOrigin,
        [(10, 90), (50, 90), (30, 130)])
    d.sync()
    passed += 1; print("PASS: FillPoly (triangle)")
except Exception as e:
    failed += 1; print(f"FAIL: FillPoly: {e}")

# Test 6: PolyFillRectangle
try:
    w.fill_rectangle(gc, 10, 140, 80, 30)
    d.sync()
    passed += 1; print("PASS: PolyFillRectangle")
except Exception as e:
    failed += 1; print(f"FAIL: PolyFillRectangle: {e}")

# Test 7: PolyArc
try:
    w.arc(gc, 110, 10, 60, 60, 0, 360*64)
    d.sync()
    passed += 1; print("PASS: PolyArc (circle)")
except Exception as e:
    failed += 1; print(f"FAIL: PolyArc: {e}")

# Test 8: PolyFillArc
try:
    w.fill_arc(gc, 110, 80, 60, 60, 0, 360*64)
    d.sync()
    passed += 1; print("PASS: PolyFillArc")
except Exception as e:
    failed += 1; print(f"FAIL: PolyFillArc: {e}")

# Test 9: PolyPoint
try:
    w.poly_point(gc, Xlib.X.CoordModeOrigin,
        [(120, 150), (130, 160), (140, 170)])
    d.sync()
    passed += 1; print("PASS: PolyPoint")
except Exception as e:
    failed += 1; print(f"FAIL: PolyPoint: {e}")

# Test 10: ClearArea
try:
    w.clear_area(10, 10, 50, 50)
    d.sync()
    passed += 1; print("PASS: ClearArea")
except Exception as e:
    failed += 1; print(f"FAIL: ClearArea: {e}")

# Test 11: ChangeGC (change foreground color)
try:
    gc.change(foreground=0xFF0000)
    w.fill_rectangle(gc, 110, 150, 30, 30)
    d.sync()
    passed += 1; print("PASS: ChangeGC + draw with new color")
except Exception as e:
    failed += 1; print(f"FAIL: ChangeGC: {e}")

# Test 12: FreeGC
try:
    gc.free()
    d.sync()
    passed += 1; print("PASS: FreeGC")
except Exception as e:
    failed += 1; print(f"FAIL: FreeGC: {e}")

w.destroy()
d.close()
print(f"xts-drawing: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
