import Xlib.display
import Xlib.X
import Xlib.Xatom
import sys

errors = []

d = Xlib.display.Display(':99')
root = d.screen().root

# Test 1: InternAtom for a new atom, then GetAtomName round-trip
atom_name = '_XTS_TEST_ATOM_ROUNDTRIP'
atom = d.intern_atom(atom_name, only_if_exists=False)
if atom == 0:
    errors.append("InternAtom returned 0 for new atom")
else:
    print(f"PASS: InternAtom returned atom id {atom}")

# GetAtomName round-trip
name_back = d.get_atom_name(atom)
if name_back != atom_name:
    errors.append(f"GetAtomName mismatch: {name_back!r} != {atom_name!r}")
else:
    print("PASS: GetAtomName round-trip matches")

# Test 2: InternAtom with only_if_exists=True for unknown atom
nonexistent = d.intern_atom('_XTS_NONEXISTENT_ATOM_12345', only_if_exists=True)
if nonexistent != 0:
    errors.append(f"InternAtom(only_if_exists=True) returned {nonexistent} for unknown atom")
else:
    print("PASS: InternAtom(only_if_exists=True) returns 0 for unknown")

# Test 3: Predefined atoms have correct IDs (X11 spec table)
predefined = {
    'PRIMARY': 1,
    'SECONDARY': 2,
    'ARC': 3,
    'ATOM': 4,
    'BITMAP': 5,
    'STRING': 31,
    'WM_NAME': 39,
    'WM_NORMAL_HINTS': 40,
}
for name, expected_id in predefined.items():
    got = d.intern_atom(name, only_if_exists=True)
    if got != expected_id:
        errors.append(f"Predefined atom {name}: expected {expected_id}, got {got}")
    else:
        print(f"PASS: predefined atom {name} = {expected_id}")

# Test 4: ChangeProperty / GetProperty / DeleteProperty round-trip
test_atom = d.intern_atom('_XTS_TEST_PROP', only_if_exists=False)
string_atom = d.intern_atom('STRING', only_if_exists=True)

# Set property
test_data = b'hello xts'
root.change_property(test_atom, string_atom, 8, test_data)
d.sync()

# Get property
prop = root.get_full_property(test_atom, string_atom)
if prop is None:
    errors.append("GetProperty returned None")
elif bytes(prop.value) != test_data:
    errors.append(f"GetProperty data mismatch: {bytes(prop.value)!r} != {test_data!r}")
else:
    print("PASS: ChangeProperty/GetProperty round-trip")

# ListProperties should include our test atom
props = root.list_properties()
if test_atom not in props:
    errors.append("ListProperties does not include test atom")
else:
    print("PASS: ListProperties includes test atom")

# DeleteProperty
root.delete_property(test_atom)
d.sync()
prop_after = root.get_full_property(test_atom, string_atom)
if prop_after is not None:
    errors.append("Property still exists after DeleteProperty")
else:
    print("PASS: DeleteProperty removes property")

# Test 5: ChangeProperty with mode=Append and mode=Prepend
append_atom = d.intern_atom('_XTS_APPEND_TEST', only_if_exists=False)
root.change_property(append_atom, string_atom, 8, b'first')
d.sync()
root.change_property(append_atom, string_atom, 8, b'_second',
                     mode=Xlib.X.PropModeAppend)
d.sync()
prop = root.get_full_property(append_atom, string_atom)
if prop is None:
    errors.append("Append property returned None")
elif bytes(prop.value) != b'first_second':
    errors.append(f"Append mismatch: {bytes(prop.value)!r}")
else:
    print("PASS: PropModeAppend works correctly")

root.change_property(append_atom, string_atom, 8, b'prefix_',
                     mode=Xlib.X.PropModePrepend)
d.sync()
prop = root.get_full_property(append_atom, string_atom)
if prop is None:
    errors.append("Prepend property returned None")
elif bytes(prop.value) != b'prefix_first_second':
    errors.append(f"Prepend mismatch: {bytes(prop.value)!r}")
else:
    print("PASS: PropModePrepend works correctly")

# Cleanup
root.delete_property(append_atom)
d.sync()

d.close()

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("XTS_PROPERTY_OK")
