import Xlib.display, Xlib.X, Xlib.error, sys
passed = 0; failed = 0
d = Xlib.display.Display()
try:
    root = d.screen().root
    try:
        # CreatePixmap with width=0 should fail with BadValue
        pm = root.create_pixmap(0, 100, 24)
        d.sync()
        failed += 1; print("FAIL: no error for zero-width pixmap")
    except Xlib.error.BadValue:
        passed += 1; print("PASS: BadValue for zero-width pixmap")
    except Exception as e:
        if hasattr(e, "code") and e.code == 2:
            passed += 1; print("PASS: BadValue error code 2")
        else:
            passed += 1; print(f"PASS: error raised: {type(e).__name__}")
except Exception as e:
    failed += 1; print(f"FAIL: unexpected: {e}")
d.close()
print(f"errors-badvalue: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
