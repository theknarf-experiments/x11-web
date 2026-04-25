import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
NET_ACTIVE = d.intern_atom("_NET_ACTIVE_WINDOW")
try:
    w1 = root.create_window(0, 0, 200, 200, 0, 24, Xlib.X.InputOutput)
    w2 = root.create_window(50, 50, 200, 200, 0, 24, Xlib.X.InputOutput)
    w1.map(); w2.map()
    d.sync()
    # Focus w1
    d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
    d.sync()
    prop = root.get_full_property(NET_ACTIVE, Xlib.X.Atom("WINDOW", d))
    if prop is not None and list(prop.value)[0] == w1.id:
        passed += 1; print(f"PASS: active={w1.id:#x}")
    else:
        failed += 1; print(f"FAIL: expected active={w1.id:#x}")
    # Focus w2
    d.set_input_focus(w2, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
    d.sync()
    prop = root.get_full_property(NET_ACTIVE, Xlib.X.Atom("WINDOW", d))
    if prop is not None and list(prop.value)[0] == w2.id:
        passed += 1; print(f"PASS: active={w2.id:#x}")
    else:
        failed += 1; print(f"FAIL: expected active={w2.id:#x}")
    w1.destroy(); w2.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"ewmh-active: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
