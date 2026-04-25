import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    # Create InputOnly window (class=2)
    w = root.create_window(0, 0, 100, 100, 0, 0, Xlib.X.InputOnly,
        event_mask=Xlib.X.KeyPressMask | Xlib.X.ButtonPressMask)
    d.sync()
    passed += 1; print("PASS: InputOnly window created")
    w.map()
    d.sync()
    passed += 1; print("PASS: InputOnly window mapped")
    # GetGeometry should work
    g = w.get_geometry()
    if g.width == 100 and g.height == 100:
        passed += 1; print(f"PASS: geometry {g.width}x{g.height}")
    else:
        failed += 1; print(f"FAIL: geometry {g.width}x{g.height}")
    # GetWindowAttributes should report class=InputOnly (2)
    a = w.get_attributes()
    if a.win_class == Xlib.X.InputOnly:
        passed += 1; print(f"PASS: class=InputOnly")
    else:
        failed += 1; print(f"FAIL: class={a.win_class}")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"inputonly: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
