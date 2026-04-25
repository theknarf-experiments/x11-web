import Xlib.display, Xlib.X, Xlib.Xatom, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)

# Test 1: ChangeProperty (string)
try:
    w.change_property(Xlib.Xatom.WM_NAME, Xlib.Xatom.STRING, 8,
        b"Test Window")
    d.sync()
    passed += 1; print("PASS: ChangeProperty WM_NAME")
except Exception as e:
    failed += 1; print(f"FAIL: ChangeProperty: {e}")

# Test 2: GetProperty (string)
try:
    prop = w.get_property(Xlib.Xatom.WM_NAME, Xlib.Xatom.STRING, 0, 100)
    val = prop.value if prop else b""
    if val == b"Test Window":
        passed += 1; print(f"PASS: GetProperty = {val}")
    else:
        failed += 1; print(f"FAIL: GetProperty = {val}")
except Exception as e:
    failed += 1; print(f"FAIL: GetProperty: {e}")

# Test 3: ListProperties includes WM_NAME
try:
    props = w.list_properties()
    if Xlib.Xatom.WM_NAME in props:
        passed += 1; print(f"PASS: ListProperties has WM_NAME ({len(props)} total)")
    else:
        failed += 1; print(f"FAIL: WM_NAME not in ListProperties")
except Exception as e:
    failed += 1; print(f"FAIL: ListProperties: {e}")

# Test 4: ChangeProperty append mode
try:
    custom_atom = d.intern_atom("XTS_TEST_PROP")
    w.change_property(custom_atom, Xlib.Xatom.STRING, 8, b"Hello")
    w.change_property(custom_atom, Xlib.Xatom.STRING, 8, b" World",
        mode=Xlib.X.PropModeAppend)
    d.sync()
    prop = w.get_property(custom_atom, Xlib.Xatom.STRING, 0, 100)
    if prop and prop.value == b"Hello World":
        passed += 1; print("PASS: PropModeAppend")
    else:
        failed += 1; print(f"FAIL: append result = {prop.value if prop else None}")
except Exception as e:
    failed += 1; print(f"FAIL: PropModeAppend: {e}")

# Test 5: ChangeProperty prepend mode
try:
    w.change_property(custom_atom, Xlib.Xatom.STRING, 8, b"Prefix ",
        mode=Xlib.X.PropModePrepend)
    d.sync()
    prop = w.get_property(custom_atom, Xlib.Xatom.STRING, 0, 100)
    if prop and prop.value == b"Prefix Hello World":
        passed += 1; print("PASS: PropModePrepend")
    else:
        failed += 1; print(f"FAIL: prepend result = {prop.value if prop else None}")
except Exception as e:
    failed += 1; print(f"FAIL: PropModePrepend: {e}")

# Test 6: DeleteProperty
try:
    w.delete_property(custom_atom)
    d.sync()
    prop = w.get_property(custom_atom, Xlib.Xatom.STRING, 0, 100)
    if prop is None or prop.property_type == 0:
        passed += 1; print("PASS: DeleteProperty")
    else:
        failed += 1; print(f"FAIL: property still exists after delete")
except Exception as e:
    failed += 1; print(f"FAIL: DeleteProperty: {e}")

# Test 7: ChangeProperty with 32-bit integer data
try:
    int_atom = d.intern_atom("XTS_INT_PROP")
    import struct
    w.change_property(int_atom, Xlib.Xatom.CARDINAL, 32, [42, 100, 255])
    d.sync()
    prop = w.get_property(int_atom, Xlib.Xatom.CARDINAL, 0, 100)
    if prop and list(prop.value) == [42, 100, 255]:
        passed += 1; print("PASS: 32-bit integer property")
    else:
        failed += 1; print(f"FAIL: int prop = {list(prop.value) if prop else None}")
except Exception as e:
    failed += 1; print(f"FAIL: int property: {e}")

w.destroy()
d.close()
print(f"xts-property: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
