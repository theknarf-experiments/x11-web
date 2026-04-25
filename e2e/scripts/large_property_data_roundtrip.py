import Xlib.display, Xlib.Xatom
d = Xlib.display.Display(":99")
root = d.screen().root
w = root.create_window(0, 0, 1, 1, 0, 0)
d.sync()
prop = d.intern_atom("_LARGE_PROP_TEST")
# Set a 64KB property
data = bytes(range(256)) * 256  # 64KB
w.change_property(prop, Xlib.Xatom.STRING, 8, data)
d.sync()
# Read it back in chunks
offset = 0
result = b""
while True:
    chunk = w.get_property(prop, Xlib.Xatom.STRING, offset, 4096)
    if chunk is None or len(chunk.value) == 0:
        break
    result += bytes(chunk.value)
    offset += len(chunk.value)
    if chunk.bytes_after == 0:
        break
assert len(result) == 65536, f"Expected 65536 bytes, got {len(result)}"
assert result == data, "Data mismatch"
print("LARGE_PROP_OK")
w.destroy()
d.sync()
d.close()
