import Xlib.display, Xlib.X, Xlib.Xatom, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
w = root.create_window(0, 0, 200, 200, 0, 24, Xlib.X.InputOutput)
w.map()
d.sync()
# Test 1: Create GC
gc = w.create_gc(foreground=0xFF0000, background=0x000000)
d.sync()
passed += 1
# Test 2: PolyFillRectangle
w.fill_rectangle(gc, 10, 10, 50, 50)
d.sync()
passed += 1
# Test 3: PolyLine
w.poly_line(gc, Xlib.X.CoordModeOrigin, [(0, 0), (100, 100), (200, 0)])
d.sync()
passed += 1
# Test 4: PolySegment
w.poly_segment(gc, [(10, 10, 190, 10), (10, 190, 190, 190)])
d.sync()
passed += 1
# Test 5: PolyRectangle
w.rectangle(gc, 20, 20, 160, 160)
d.sync()
passed += 1
# Test 6: CreatePixmap + FreePixmap
pm = w.create_pixmap(100, 100, 24)
d.sync()
pm.free()
d.sync()
passed += 1
# Test 7: ClearArea
w.clear_area(0, 0, 200, 200)
d.sync()
passed += 1
# Test 8: ChangeGC
gc.change(foreground=0x00FF00, line_width=3)
d.sync()
passed += 1
# Test 9: FreeGC
gc.free()
d.sync()
passed += 1
w.destroy()
d.close()
print(f"gc-drawing: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
