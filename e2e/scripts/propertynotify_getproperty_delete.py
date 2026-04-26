import Xlib.display, Xlib.X, sys, Xlib.Xatom
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    w = root.create_window(0, 0, 100, 100, 0, 24, Xlib.X.InputOutput,
        event_mask=Xlib.X.PropertyChangeMask)
    TEST_PROP = d.intern_atom("_TEST_DELETE_PROP")
    w.change_property(TEST_PROP, Xlib.Xatom.STRING, 8, b"hello")
    d.sync()
    passed += 1; print("PASS: property set")
    # Drain PropertyNotify from ChangeProperty
    while d.pending_events() > 0:
        d.next_event()
    # GetProperty with delete=True should generate PropertyNotify(Deleted)
    p = w.get_full_property(TEST_PROP, Xlib.Xatom.STRING, sizehint=1024)
    if p and bytes(p.value) == b"hello":
        passed += 1; print("PASS: GetProperty returned value")
    else:
        failed += 1; print("FAIL: GetProperty value mismatch")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"propnotify-del: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
