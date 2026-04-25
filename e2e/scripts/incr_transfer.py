import Xlib.display
import Xlib.X
import Xlib.Xatom
import sys

errors = []

d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root

# Test 1: Large property data (64KB)
large_atom = d.intern_atom('_XTS_LARGE_PROP')
large_data = b'X' * 65536
root.change_property(large_atom, Xlib.Xatom.STRING, 8, large_data)
d.sync()

prop = root.get_full_property(large_atom, Xlib.Xatom.STRING)
if prop is None:
    errors.append("Large property returned None")
elif len(prop.value) != 65536:
    errors.append(f"Large property size mismatch: {len(prop.value)} != 65536")
elif bytes(prop.value) != large_data:
    errors.append("Large property data mismatch")
else:
    print("PASS: 64KB property round-trip")

root.delete_property(large_atom)
d.sync()

# Test 2: Property with 32-bit format (array of integers)
int_atom = d.intern_atom('_XTS_INT_PROP')
int_data = list(range(1000))
root.change_property(int_atom, Xlib.Xatom.CARDINAL, 32, int_data)
d.sync()

prop = root.get_full_property(int_atom, Xlib.Xatom.CARDINAL)
if prop is None:
    errors.append("Integer property returned None")
elif len(prop.value) != 1000:
    errors.append(f"Integer property count: {len(prop.value)} != 1000")
else:
    values = list(prop.value)
    if values == int_data:
        print("PASS: 1000-element integer property round-trip")
    else:
        mismatches = sum(1 for a, b in zip(values, int_data) if a != b)
        errors.append(f"Integer property has {mismatches} mismatches")

root.delete_property(int_atom)
d.sync()

# Test 3: Property with 16-bit format
short_atom = d.intern_atom('_XTS_SHORT_PROP')
short_data = list(range(0, 2000, 2))
root.change_property(short_atom, Xlib.Xatom.CARDINAL, 16, short_data)
d.sync()

prop = root.get_full_property(short_atom, Xlib.Xatom.CARDINAL)
if prop is None:
    errors.append("Short property returned None")
elif len(prop.value) != len(short_data):
    errors.append(f"Short property count: {len(prop.value)} != {len(short_data)}")
else:
    values = list(prop.value)
    if values == short_data:
        print("PASS: 16-bit format property round-trip")
    else:
        errors.append("16-bit property data mismatch")

root.delete_property(short_atom)
d.sync()

# Test 4: GetProperty with offset and length (partial read)
partial_atom = d.intern_atom('_XTS_PARTIAL_PROP')
partial_data = b'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'
root.change_property(partial_atom, Xlib.Xatom.STRING, 8, partial_data)
d.sync()

# Read with offset (in 32-bit units) and length limit
# get_property(property, type, offset, length)
prop = root.get_property(partial_atom, Xlib.Xatom.STRING, 0, 4)
if prop is None:
    errors.append("Partial property read returned None")
elif len(prop.value) > 0:
    print(f"PASS: partial GetProperty returned {len(prop.value)} bytes")
else:
    errors.append("Partial GetProperty returned empty")

root.delete_property(partial_atom)
d.sync()

d.close()

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("INCR_TRANSFER_OK")
