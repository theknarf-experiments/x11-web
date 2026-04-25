import Xlib.display, Xlib.X, Xlib.error, sys
passed = 0; failed = 0
d = Xlib.display.Display()
try:
    try:
        name = d.get_atom_name(99999)
        d.sync()
        failed += 1; print("FAIL: no error for invalid atom")
    except Xlib.error.BadAtom:
        passed += 1; print("PASS: BadAtom for invalid atom ID")
    except Exception as e:
        if hasattr(e, "code") and e.code == 5:
            passed += 1; print("PASS: BadAtom error code 5")
        else:
            passed += 1; print(f"PASS: error raised: {type(e).__name__}")
except Exception as e:
    failed += 1; print(f"FAIL: unexpected: {e}")
d.close()
print(f"errors-badatom: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
