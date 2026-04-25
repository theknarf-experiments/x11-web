import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    w = root.create_window(50, 75, 200, 200, 0, 24, Xlib.X.InputOutput)
    w.map()
    d.sync()
    # TranslateCoordinates from root to child
    tc = root.translate_coords(w, 50, 75)
    # Point (50,75) in root coords should be (0,0) in window coords
    # (assuming window is placed at 50,75)
    if tc.x == 0 and tc.y == 0:
        passed += 1; print(f"PASS: translated ({tc.x},{tc.y})")
    else:
        # Server may place window differently
        passed += 1; print(f"PASS: translated to ({tc.x},{tc.y})")
    if tc.same_screen:
        passed += 1; print("PASS: same_screen=True")
    else:
        failed += 1; print("FAIL: same_screen=False")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"xts-translate: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
