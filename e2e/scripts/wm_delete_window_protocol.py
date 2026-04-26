import Xlib.display, Xlib.X, Xlib.Xutil, sys, time, Xlib.Xatom
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
WM_PROTOCOLS = d.intern_atom("WM_PROTOCOLS")
WM_DELETE_WINDOW = d.intern_atom("WM_DELETE_WINDOW")
try:
    w = root.create_window(0, 0, 200, 200, 0, 24, Xlib.X.InputOutput)
    # Set WM_PROTOCOLS to include WM_DELETE_WINDOW
    w.change_property(WM_PROTOCOLS, Xlib.Xatom.ATOM, 32, [WM_DELETE_WINDOW])
    w.map()
    d.sync()
    passed += 1; print("PASS: set WM_PROTOCOLS with WM_DELETE_WINDOW")
    # Read back WM_PROTOCOLS
    prop = w.get_full_property(WM_PROTOCOLS, Xlib.Xatom.ATOM)
    if prop and WM_DELETE_WINDOW in list(prop.value):
        passed += 1; print("PASS: WM_DELETE_WINDOW in WM_PROTOCOLS")
    else:
        failed += 1; print("FAIL: WM_DELETE_WINDOW not in WM_PROTOCOLS")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"icccm-delete: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
