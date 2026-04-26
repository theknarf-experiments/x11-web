import Xlib.display, Xlib.X, Xlib.Xutil, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    w = root.create_window(0, 0, 300, 200, 0, 24, Xlib.X.InputOutput)
    # Set WM_NORMAL_HINTS with min/max sizes (kwargs form, since
    # python-xlib's set_wm_normal_hints doesn't accept a class instance)
    w.set_wm_normal_hints(
        flags=Xlib.Xutil.PMinSize | Xlib.Xutil.PMaxSize | Xlib.Xutil.PResizeInc,
        min_width=100, min_height=80,
        max_width=800, max_height=600,
        width_inc=10, height_inc=10,
    )
    d.sync()
    passed += 1; print("PASS: set WM_NORMAL_HINTS")
    # Read back
    h = w.get_wm_normal_hints()
    if h is not None:
        if h.min_width == 100 and h.min_height == 80:
            passed += 1; print(f"PASS: min_size={h.min_width}x{h.min_height}")
        else:
            failed += 1; print(f"FAIL: min_size={h.min_width}x{h.min_height}")
        if h.max_width == 800 and h.max_height == 600:
            passed += 1; print(f"PASS: max_size={h.max_width}x{h.max_height}")
        else:
            failed += 1; print(f"FAIL: max_size={h.max_width}x{h.max_height}")
    else:
        failed += 1; print("FAIL: WM_NORMAL_HINTS not returned")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"icccm-hints: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
