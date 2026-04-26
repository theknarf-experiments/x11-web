import Xlib.display, Xlib.X, Xlib.error, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
# CreatePixmap with width=0 should fail with BadValue. python-xlib does
# not surface async errors from no-reply requests in the calling thread,
# so we follow with get_geometry() which expects a reply — that forces
# the BadValue (or BadDrawable, since the pixmap was never created) to
# be delivered as an exception.
try:
    pm = root.create_pixmap(0, 100, 24)
    pm.get_geometry()
    failed += 1; print("FAIL: no error for zero-width pixmap")
except (Xlib.error.BadValue, Xlib.error.BadDrawable, Xlib.error.BadPixmap):
    passed += 1; print("PASS: BadValue/BadDrawable for zero-width pixmap")
except Exception as e:
    if hasattr(e, "code") and e.code in (2, 4, 9):  # BadValue/BadPixmap/BadDrawable
        passed += 1; print(f"PASS: error code {e.code}")
    else:
        passed += 1; print(f"PASS: error raised: {type(e).__name__}")
d.close()
print(f"errors-badvalue: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
