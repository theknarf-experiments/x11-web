import Xlib.display, Xlib.X, sys, Xlib.Xatom
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    w = root.create_window(0, 0, 100, 100, 0, 24, Xlib.X.InputOutput)
    a1 = d.intern_atom("_LP_TEST_1")
    a2 = d.intern_atom("_LP_TEST_2")
    w.change_property(a1, Xlib.Xatom.STRING, 8, b"one")
    w.change_property(a2, Xlib.Xatom.STRING, 8, b"two")
    d.sync()
    props = w.list_properties()
    prop_ids = [p.id if hasattr(p, "id") else p for p in props]
    if a1 in prop_ids and a2 in prop_ids:
        passed += 1; print(f"PASS: both properties listed ({len(prop_ids)} total)")
    else:
        failed += 1; print(f"FAIL: properties not found in list")
    # DeleteProperty
    w.delete_property(a1)
    d.sync()
    props2 = w.list_properties()
    prop_ids2 = [p.id if hasattr(p, "id") else p for p in props2]
    if a1 not in prop_ids2:
        passed += 1; print("PASS: deleted property removed from list")
    else:
        failed += 1; print("FAIL: deleted property still in list")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"xts-listprops: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
