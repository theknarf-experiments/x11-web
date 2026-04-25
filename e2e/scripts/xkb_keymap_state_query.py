import Xlib.display, sys
passed = 0; failed = 0
d = Xlib.display.Display()

# Test QueryExtension for XKEYBOARD
ext = d.query_extension("XKEYBOARD")
if ext and ext.present:
    passed += 1; print(f"PASS: XKB present, opcode={ext.major_opcode}")
else:
    failed += 1; print("FAIL: XKB not present")

# Test keyboard mapping is populated
km = d.get_keyboard_mapping(8, 248)
if km and len(km) > 0:
    non_zero = sum(1 for row in km for ks in row if ks != 0)
    if non_zero > 50:
        passed += 1; print(f"PASS: keyboard mapping has {non_zero} non-zero keysyms")
    else:
        failed += 1; print(f"FAIL: only {non_zero} non-zero keysyms")
else:
    failed += 1; print("FAIL: empty keyboard mapping")

# Test modifier mapping
mm = d.get_modifier_mapping()
if mm and len(mm) == 8:
    passed += 1; print(f"PASS: modifier mapping has 8 rows")
else:
    failed += 1; print(f"FAIL: modifier mapping: {mm}")

d.close()
print(f"xkb: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
