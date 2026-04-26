import Xlib.display
import Xlib.X
import Xlib.Xatom
import sys
import hashlib

d = Xlib.display.Display(':99')
root = d.screen().root

# Create a test atom
test_atom = d.intern_atom('_X11WEB_LARGE_PROP_TEST', only_if_exists=False)

# Generate 1MB of deterministic data
# Use 8-bit format (bytes), 1048576 bytes = 1MB
size = 1024 * 1024
data = bytes(range(256)) * (size // 256)
expected_hash = hashlib.sha256(data).hexdigest()
print(f"PASS: generated {len(data)} bytes, sha256={expected_hash[:16]}...")

# Set the property. ChangeProperty's length field is a 16-bit count of
# 32-bit words, so a single request maxes out around 256KB; we chunk
# 1MB across Replace + multiple Append calls (BIG-REQUESTS would also
# work, but Append is a portable fallback).
CHUNK = 128 * 1024
root.change_property(test_atom, Xlib.Xatom.STRING, 8, data[:CHUNK],
                     mode=Xlib.X.PropModeReplace)
for off in range(CHUNK, len(data), CHUNK):
    root.change_property(test_atom, Xlib.Xatom.STRING, 8,
                         data[off:off + CHUNK],
                         mode=Xlib.X.PropModeAppend)
d.sync()
print("PASS: ChangeProperty with 1MB data completed")

# Read it back
prop = root.get_full_property(test_atom, Xlib.Xatom.STRING)
if prop is None:
    print("FAIL: property not found after setting")
    sys.exit(1)

read_data = bytes(prop.value)
actual_hash = hashlib.sha256(read_data).hexdigest()
print(f"PASS: read back {len(read_data)} bytes, sha256={actual_hash[:16]}...")

if len(read_data) != len(data):
    print(f"FAIL: size mismatch: wrote {len(data)} but read {len(read_data)}")
    sys.exit(1)

if actual_hash != expected_hash:
    print("FAIL: data corruption detected")
    sys.exit(1)

print("PASS: 1MB property data verified")

# Clean up
root.delete_property(test_atom)
d.sync()

d.close()
print("LARGE_PROPERTY_OK")
