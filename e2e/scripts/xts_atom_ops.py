import Xlib.display, Xlib.X, Xlib.Xatom, sys
passed = 0; failed = 0
d = Xlib.display.Display()

# Test 1: predefined atoms have correct values
try:
    name = d.get_atom_name(Xlib.Xatom.PRIMARY)
    if name == "PRIMARY":
        passed += 1; print("PASS: atom 1 = PRIMARY")
    else:
        failed += 1; print(f"FAIL: atom 1 = {name}")
except Exception as e:
    failed += 1; print(f"FAIL: GetAtomName PRIMARY: {e}")

# Test 2: WM_NAME atom
try:
    name = d.get_atom_name(Xlib.Xatom.WM_NAME)
    if name == "WM_NAME":
        passed += 1; print("PASS: atom 39 = WM_NAME")
    else:
        failed += 1; print(f"FAIL: atom 39 = {name}")
except Exception as e:
    failed += 1; print(f"FAIL: GetAtomName WM_NAME: {e}")

# Test 3: InternAtom creates new atom
try:
    atom_id = d.intern_atom("XTS_UNIQUE_ATOM_12345")
    if atom_id > 0:
        passed += 1; print(f"PASS: InternAtom created id={atom_id}")
    else:
        failed += 1; print(f"FAIL: InternAtom returned {atom_id}")
except Exception as e:
    failed += 1; print(f"FAIL: InternAtom: {e}")

# Test 4: GetAtomName round-trips
try:
    name = d.get_atom_name(atom_id)
    if name == "XTS_UNIQUE_ATOM_12345":
        passed += 1; print("PASS: GetAtomName round-trip")
    else:
        failed += 1; print(f"FAIL: round-trip got {name}")
except Exception as e:
    failed += 1; print(f"FAIL: GetAtomName round-trip: {e}")

# Test 5: InternAtom only_if_exists=True for unknown atom
try:
    atom_id2 = d.intern_atom("XTS_NONEXISTENT_99999", only_if_exists=True)
    if atom_id2 == 0:
        passed += 1; print("PASS: only_if_exists returns None/0 for unknown")
    else:
        failed += 1; print(f"FAIL: only_if_exists returned {atom_id2}")
except Exception as e:
    failed += 1; print(f"FAIL: InternAtom only_if_exists: {e}")

# Test 6: InternAtom only_if_exists=True for known atom
try:
    atom_id3 = d.intern_atom("XTS_UNIQUE_ATOM_12345", only_if_exists=True)
    if atom_id3 == atom_id:
        passed += 1; print(f"PASS: only_if_exists returns {atom_id3} for known atom")
    else:
        failed += 1; print(f"FAIL: only_if_exists returned {atom_id3}, expected {atom_id}")
except Exception as e:
    failed += 1; print(f"FAIL: InternAtom only_if_exists known: {e}")

# Test 7: Multiple InternAtom calls return same id
try:
    atom_id4 = d.intern_atom("XTS_UNIQUE_ATOM_12345")
    if atom_id4 == atom_id:
        passed += 1; print("PASS: InternAtom is idempotent")
    else:
        failed += 1; print(f"FAIL: second InternAtom returned {atom_id4}")
except Exception as e:
    failed += 1; print(f"FAIL: InternAtom idempotent: {e}")

# Test 8: Batch of predefined atoms
predefined = {
    Xlib.Xatom.SECONDARY: "SECONDARY",
    Xlib.Xatom.ATOM: "ATOM",
    Xlib.Xatom.WINDOW: "WINDOW",
    Xlib.Xatom.WM_CLASS: "WM_CLASS",
    Xlib.Xatom.WM_COMMAND: "WM_COMMAND",
}
all_ok = True
for aid, expected in predefined.items():
    name = d.get_atom_name(aid)
    if name != expected:
        all_ok = False; failed += 1
        print(f"FAIL: atom {aid} = {name}, expected {expected}")
if all_ok:
    passed += 1; print("PASS: 5 predefined atoms verified")

d.close()
print(f"xts-atom: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
