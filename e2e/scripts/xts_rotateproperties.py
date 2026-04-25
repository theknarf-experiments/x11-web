import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    w = root.create_window(0, 0, 100, 100, 0, 24, Xlib.X.InputOutput)
    # Set three properties
    a1 = d.intern_atom("_TEST_PROP_A")
    a2 = d.intern_atom("_TEST_PROP_B")
    a3 = d.intern_atom("_TEST_PROP_C")
    w.change_property(a1, Xlib.X.Atom("STRING", d), 8, b"alpha")
    w.change_property(a2, Xlib.X.Atom("STRING", d), 8, b"beta")
    w.change_property(a3, Xlib.X.Atom("STRING", d), 8, b"gamma")
    d.sync()
    passed += 1; print("PASS: set 3 properties")
    # Rotate: shift by 1
    w.rotate_properties([a1, a2, a3], 1)
    d.sync()
    passed += 1; print("PASS: RotateProperties completed")
    # After rotate by 1: a1 should have gamma, a2 alpha, a3 beta
    p1 = w.get_full_property(a1, Xlib.X.Atom("STRING", d))
    p2 = w.get_full_property(a2, Xlib.X.Atom("STRING", d))
    p3 = w.get_full_property(a3, Xlib.X.Atom("STRING", d))
    v1 = bytes(p1.value) if p1 else b""
    v2 = bytes(p2.value) if p2 else b""
    v3 = bytes(p3.value) if p3 else b""
    if v1 == b"gamma":
        passed += 1; print(f"PASS: a1={v1}")
    else:
        failed += 1; print(f"FAIL: a1={v1} expected gamma")
    if v2 == b"alpha":
        passed += 1; print(f"PASS: a2={v2}")
    else:
        failed += 1; print(f"FAIL: a2={v2} expected alpha")
    if v3 == b"beta":
        passed += 1; print(f"PASS: a3={v3}")
    else:
        failed += 1; print(f"FAIL: a3={v3} expected beta")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"xts-rotate: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
