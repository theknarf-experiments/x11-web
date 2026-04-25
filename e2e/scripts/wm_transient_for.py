import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    parent = root.create_window(0, 0, 200, 200, 0, 24, Xlib.X.InputOutput)
    child = root.create_window(50, 50, 100, 100, 0, 24, Xlib.X.InputOutput)
    # Set transient-for
    child.set_wm_transient_for(parent)
    d.sync()
    passed += 1; print("PASS: set WM_TRANSIENT_FOR")
    # Read back
    t = child.get_wm_transient_for()
    if t is not None and t.id == parent.id:
        passed += 1; print(f"PASS: transient_for={t.id:#x} == parent={parent.id:#x}")
    else:
        tid = t.id if t else None
        failed += 1; print(f"FAIL: transient_for={tid} != parent={parent.id:#x}")
    child.destroy()
    parent.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"icccm-transient: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
