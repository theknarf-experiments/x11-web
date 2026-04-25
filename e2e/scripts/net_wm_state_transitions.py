import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
NET_WM_STATE = d.intern_atom("_NET_WM_STATE")
NET_WM_STATE_MAXIMIZED_VERT = d.intern_atom("_NET_WM_STATE_MAXIMIZED_VERT")
NET_WM_STATE_MAXIMIZED_HORZ = d.intern_atom("_NET_WM_STATE_MAXIMIZED_HORZ")
NET_WM_STATE_FULLSCREEN = d.intern_atom("_NET_WM_STATE_FULLSCREEN")
try:
    w = root.create_window(0, 0, 200, 200, 0, 24, Xlib.X.InputOutput)
    w.map()
    d.sync()
    # Set fullscreen state via property
    w.change_property(NET_WM_STATE, Xlib.X.Atom("ATOM", d), 32,
        [NET_WM_STATE_FULLSCREEN])
    d.sync()
    passed += 1; print("PASS: set _NET_WM_STATE_FULLSCREEN")
    # Read back state
    prop = w.get_full_property(NET_WM_STATE, Xlib.X.Atom("ATOM", d))
    if prop is not None:
        atoms = list(prop.value)
        if NET_WM_STATE_FULLSCREEN in atoms:
            passed += 1; print("PASS: fullscreen state readable")
        else:
            failed += 1; print(f"FAIL: fullscreen not in state {atoms}")
    else:
        failed += 1; print("FAIL: _NET_WM_STATE property not found")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"ewmh-state: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
