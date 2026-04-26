import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
try:
    d = Xlib.display.Display()
    # Test 1: connection succeeds
    passed += 1
    print("PASS: connection established")
    # Test 2: protocol version
    v = d.display.info.protocol_major_version
    if v == 11:
        passed += 1; print(f"PASS: protocol version {v}")
    else:
        failed += 1; print(f"FAIL: protocol version {v}, expected 11")
    # Test 3: screen count >= 1
    sc = d.screen_count()
    if sc >= 1:
        passed += 1; print(f"PASS: screen count {sc}")
    else:
        failed += 1; print(f"FAIL: screen count {sc}")
    # Test 4: root window exists
    root = d.screen().root
    if root.id > 0:
        passed += 1; print(f"PASS: root window id 0x{root.id:x}")
    else:
        failed += 1; print("FAIL: invalid root window id")
    # Test 5: root has valid geometry
    geom = root.get_geometry()
    if geom.width > 0 and geom.height > 0:
        passed += 1; print(f"PASS: root geometry {geom.width}x{geom.height}")
    else:
        failed += 1; print(f"FAIL: root geometry {geom.width}x{geom.height}")
    # Test 6: root depth is valid (typically 24 or 32)
    if geom.depth >= 24:
        passed += 1; print(f"PASS: root depth {geom.depth}")
    else:
        failed += 1; print(f"FAIL: root depth {geom.depth}")
    # Test 7: vendor string is non-empty
    vendor = d.display.info.vendor
    if len(vendor) > 0:
        passed += 1; print(f"PASS: vendor = {vendor}")
    else:
        failed += 1; print("FAIL: empty vendor string")
    d.close()
except Exception as e:
    failed += 1; print(f"FAIL: exception {e}")
print(f"xts-connection: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
