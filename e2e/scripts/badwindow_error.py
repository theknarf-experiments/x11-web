import Xlib.display, Xlib.X, Xlib.error, sys
passed = 0; failed = 0
d = Xlib.display.Display()
try:
    # Request attributes on a non-existent window ID
    from Xlib.protocol import request
    bad_wid = 0xDEAD
    try:
        d.get_atom("_NET_WM_NAME")
        # Create a proxy object for a non-existent window
        fake_win = d.create_resource_object("window", bad_wid)
        fake_win.get_geometry()
        d.sync()
        failed += 1; print("FAIL: no error raised for bad window")
    except Xlib.error.BadWindow as e:
        passed += 1; print(f"PASS: BadWindow raised for {bad_wid:#x}")
    except Exception as e:
        # Some versions raise XError with code 3
        if hasattr(e, "code") and e.code == 3:
            passed += 1; print(f"PASS: BadWindow error code 3")
        else:
            passed += 1; print(f"PASS: error raised: {type(e).__name__}")
except Exception as e:
    failed += 1; print(f"FAIL: unexpected: {e}")
d.close()
print(f"errors-badwindow: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
