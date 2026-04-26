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
# Read it back in chunks. X11 GetProperty offset/length are in 32-bit
# words, NOT bytes — and (chunk.value) is bytes for STRING data, so we
# have to track the word offset separately.
offset_words = 0
chunk_words = 4096  # 16 KB per request
result = b""
while True:
    chunk = w.get_property(prop, Xlib.Xatom.STRING, offset_words, chunk_words)
    if chunk is None or len(chunk.value) == 0:
        break
    result += bytes(chunk.value)
    # Each STRING byte counts as 1 unit of value, but the offset/length
    # advance is still in 4-byte words.
    offset_words += (len(chunk.value) + 3) // 4
    if chunk.bytes_after == 0:
        break
assert len(result) == 65536, f"Expected 65536 bytes, got {len(result)}"
assert result == data, "Data mismatch"
print("LARGE_PROP_OK")
w.destroy()
d.sync()
d.close()
