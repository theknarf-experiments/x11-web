import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
s = d.screen()
root = s.root

# Create a window for drawing
w = root.create_window(0, 0, 200, 200, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Test CreateGC
try:
    gc = w.create_gc(foreground=s.white_pixel, background=s.black_pixel, line_width=2)
    passed += 1; print("PASS: CreateGC")
except Exception as e:
    failed += 1; print(f"FAIL: CreateGC: {e}")
    sys.exit(1)

# Test drawing operations
try:
    w.fill_rectangle(gc, 10, 10, 50, 50)
    passed += 1; print("PASS: FillRectangle")
except Exception as e:
    failed += 1; print(f"FAIL: FillRectangle: {e}")

try:
    w.poly_line(gc, Xlib.X.CoordModeOrigin, [(0, 0), (100, 100), (100, 0)])
    passed += 1; print("PASS: PolyLine")
except Exception as e:
    failed += 1; print(f"FAIL: PolyLine: {e}")

try:
    w.poly_segment(gc, [(0, 0, 50, 50), (50, 0, 0, 50)])
    passed += 1; print("PASS: PolySegment")
except Exception as e:
    failed += 1; print(f"FAIL: PolySegment: {e}")

try:
    w.draw_arc(gc, 20, 20, 60, 60, 0, 360*64)
    passed += 1; print("PASS: PolyArc")
except Exception as e:
    failed += 1; print(f"FAIL: PolyArc: {e}")

try:
    w.fill_arc(gc, 20, 20, 60, 60, 0, 360*64)
    passed += 1; print("PASS: FillArc")
except Exception as e:
    failed += 1; print(f"FAIL: FillArc: {e}")

try:
    w.poly_point(gc, Xlib.X.CoordModeOrigin, [(5, 5), (10, 10), (15, 15)])
    passed += 1; print("PASS: PolyPoint")
except Exception as e:
    failed += 1; print(f"FAIL: PolyPoint: {e}")

try:
    w.poly_rectangle(gc, [(10, 10, 30, 30), (50, 50, 40, 40)])
    passed += 1; print("PASS: PolyRectangle")
except Exception as e:
    failed += 1; print(f"FAIL: PolyRectangle: {e}")

d.sync()

# Test CopyArea
try:
    gc2 = w.create_gc()
    w.copy_area(gc2, w, 0, 0, 50, 50, 100, 100)
    passed += 1; print("PASS: CopyArea")
except Exception as e:
    failed += 1; print(f"FAIL: CopyArea: {e}")

d.sync()
gc.free()
w.destroy()
d.close()

print(f"drawing: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
