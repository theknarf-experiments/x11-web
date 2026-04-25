import Xlib.display, Xlib.X, Xlib.Xcursorfont, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

# Test 1: Create cursor from font
try:
    font = d.open_font("cursor")
    cursor = font.create_glyph_cursor(
        font, Xlib.Xcursorfont.left_ptr, Xlib.Xcursorfont.left_ptr + 1,
        (0, 0, 0), (65535, 65535, 65535))
    passed += 1; print("PASS: create glyph cursor")
except Exception as e:
    failed += 1; print(f"FAIL: create glyph cursor: {e}")

# Test 2: Define cursor on window
try:
    w = root.create_window(0, 0, 50, 50, 0,
        d.screen().root_depth,
        Xlib.X.InputOutput, Xlib.X.CopyFromParent,
        cursor=cursor)
    w.map()
    d.sync()
    passed += 1; print("PASS: define cursor on window")
except Exception as e:
    failed += 1; print(f"FAIL: define cursor on window: {e}")

# Test 3: Change cursor via ChangeWindowAttributes
try:
    font2 = d.open_font("cursor")
    cursor2 = font2.create_glyph_cursor(
        font2, Xlib.Xcursorfont.crosshair, Xlib.Xcursorfont.crosshair + 1,
        (65535, 0, 0), (0, 0, 0))
    w.change_attributes(cursor=cursor2)
    d.sync()
    passed += 1; print("PASS: change cursor via ChangeWindowAttributes")
except Exception as e:
    failed += 1; print(f"FAIL: change cursor: {e}")

# Test 4: Free cursor (should not error)
try:
    cursor.free(onerror=None)
    cursor2.free(onerror=None)
    d.sync()
    passed += 1; print("PASS: free cursors")
except Exception as e:
    failed += 1; print(f"FAIL: free cursors: {e}")

w.destroy()
d.close()
print(f"xts-cursor-ops: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
