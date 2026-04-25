import Xlib.display, Xlib.X, Xlib.Xatom, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
test_atom = d.intern_atom("_TEST_PROP_MODES")
try:
    # 1. Replace mode: set initial value
    root.change_property(test_atom, Xlib.Xatom.STRING, 8, b"hello")
    d.sync()
    val = root.get_full_property(test_atom, Xlib.Xatom.STRING)
    if val and val.value == b"hello":
        passed += 1; print("PASS: Replace mode")
    else:
        failed += 1; print(f"FAIL: Replace mode got {val}")
    # 2. Append mode: add to end
    root.change_property(test_atom, Xlib.Xatom.STRING, 8, b" world", mode=Xlib.X.PropModeAppend)
    d.sync()
    val = root.get_full_property(test_atom, Xlib.Xatom.STRING)
    if val and val.value == b"hello world":
        passed += 1; print("PASS: Append mode")
    else:
        failed += 1; print(f"FAIL: Append mode got {val.value if val else None}")
    # 3. Prepend mode: add to beginning
    root.change_property(test_atom, Xlib.Xatom.STRING, 8, b"say: ", mode=Xlib.X.PropModePrepend)
    d.sync()
    val = root.get_full_property(test_atom, Xlib.Xatom.STRING)
    if val and val.value == b"say: hello world":
        passed += 1; print("PASS: Prepend mode")
    else:
        failed += 1; print(f"FAIL: Prepend mode got {val.value if val else None}")
    # Cleanup
    root.delete_property(test_atom)
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"xts-prop-modes: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
