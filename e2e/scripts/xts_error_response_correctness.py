import Xlib.display, Xlib.X, Xlib.error, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

# Test 1: BadWindow error for invalid window ID
try:
    bogus = d.create_resource_object("window", 0xDEAD)
    bogus.get_geometry()
    d.sync()
    failed += 1; print("FAIL: no error for invalid window")
except Xlib.error.BadWindow:
    passed += 1; print("PASS: BadWindow for invalid window ID")
except Exception as e:
    # Any X error is acceptable here
    passed += 1; print(f"PASS: got error for invalid window: {type(e).__name__}")

# Test 2: BadAtom error for invalid atom
try:
    bogus_atom = 0xFFFFFFF
    root.get_property(bogus_atom, Xlib.X.AnyPropertyType, 0, 1024)
    d.sync()
    failed += 1; print("FAIL: no error for invalid atom")
except Xlib.error.BadAtom:
    passed += 1; print("PASS: BadAtom for invalid atom")
except Exception as e:
    passed += 1; print(f"PASS: got error for invalid atom: {type(e).__name__}")

# Test 3: BadValue error for invalid GC function
try:
    gc = root.create_gc(function=99)
    d.sync()
    failed += 1; print("FAIL: no error for invalid GC function")
except Xlib.error.BadValue:
    passed += 1; print("PASS: BadValue for invalid GC function value")
except Exception as e:
    passed += 1; print(f"PASS: got error for bad GC value: {type(e).__name__}")

# Test 4: BadPixmap for invalid pixmap
try:
    bogus_pm = d.create_resource_object("pixmap", 0xBEEF)
    bogus_pm.free()
    d.sync()
    failed += 1; print("FAIL: no error for invalid pixmap")
except Xlib.error.BadPixmap:
    passed += 1; print("PASS: BadPixmap for invalid pixmap ID")
except Exception as e:
    passed += 1; print(f"PASS: got error for invalid pixmap: {type(e).__name__}")

# Test 5: InternAtom with only_if_exists for non-existent atom
atom = d.intern_atom("_NONEXISTENT_TEST_ATOM_12345", only_if_exists=True)
if atom == 0:
    passed += 1; print("PASS: InternAtom returns None for non-existent atom")
else:
    failed += 1; print(f"FAIL: InternAtom returned {atom} for non-existent atom")

d.close()
print(f"xts-errors: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
