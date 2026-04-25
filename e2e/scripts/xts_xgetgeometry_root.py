import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
# XGetGeometry on root
g = root.get_geometry()
if g.x == 0 and g.y == 0:
    passed += 1; print(f"PASS: root at (0,0)")
else:
    failed += 1; print(f"FAIL: root at ({g.x},{g.y})")
if g.width == 1024 and g.height == 768:
    passed += 1; print(f"PASS: root size {g.width}x{g.height}")
elif g.width > 0 and g.height > 0:
    passed += 1; print(f"PASS: root size {g.width}x{g.height} (non-default)")
else:
    failed += 1; print(f"FAIL: root size {g.width}x{g.height}")
if g.depth >= 24:
    passed += 1; print(f"PASS: root depth {g.depth}")
else:
    failed += 1; print(f"FAIL: root depth {g.depth}")
if g.border_width == 0:
    passed += 1; print("PASS: root border_width=0")
else:
    failed += 1; print(f"FAIL: root border_width={g.border_width}")
d.close()
print(f"xts-getgeom: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
