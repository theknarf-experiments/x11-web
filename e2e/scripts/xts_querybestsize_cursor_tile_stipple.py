import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
# QueryBestSize is a per-drawable request in python-xlib —
# Drawable.query_best_size(class_, width, height), not a method on
# Display. The class enum is Cursor=0, Tile=1, Stipple=2.
for class_id, name, w, h in [(0, "cursor", 32, 32), (1, "tile", 16, 16), (2, "stipple", 8, 8)]:
    try:
        reply = root.query_best_size(class_id, w, h)
        if reply.width > 0 and reply.height > 0:
            passed += 1; print(f"PASS: {name} best={reply.width}x{reply.height}")
        else:
            failed += 1; print(f"FAIL: {name} best={reply.width}x{reply.height}")
    except Exception as e:
        failed += 1; print(f"FAIL: {name}: {type(e).__name__}: {e}")
d.close()
print(f"xts-bestsize: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
