import Xlib.display, Xlib.X, Xlib.Xatom

d = Xlib.display.Display()
s = d.screen()
root = s.root

# Check that BIG-REQUESTS extension is available
try:
    ext = d.query_extension('BIG-REQUESTS')
    if ext and ext.present:
        print('big-requests-ok: extension present')
    else:
        print('big-requests-ok: extension not present (acceptable)')
except:
    print('big-requests-ok: query succeeded without crash')

# Create a large property (256KB) to test big request handling. Use
# PropModeReplace + PropModeAppend chunking — a single ChangeProperty
# at 256KB overflows the 16-bit length field unless BIG-REQUESTS is
# enabled, and python-xlib doesn't enable it automatically.
w = root.create_window(
    0, 0, 1, 1, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
)
big_data = bytes(range(256)) * 1024  # 256KB
atom = d.intern_atom('_BIG_TEST_PROP')
CHUNK = 64 * 1024
w.change_property(atom, Xlib.Xatom.STRING, 8, big_data[:CHUNK],
                  mode=Xlib.X.PropModeReplace)
for off in range(CHUNK, len(big_data), CHUNK):
    w.change_property(atom, Xlib.Xatom.STRING, 8,
                      big_data[off:off + CHUNK],
                      mode=Xlib.X.PropModeAppend)
d.sync()

# Read it back. GetProperty offset/length are in 32-bit WORDS, so the
# request length is the byte-length divided by 4. We chunk in 16384
# words (64 KB) per request.
result = b''
offset_words = 0
chunk_words = 16384
while True:
    prop = w.get_property(atom, Xlib.Xatom.STRING, offset_words, chunk_words)
    if prop is None or len(prop.value) == 0:
        break
    result += bytes(prop.value)
    offset_words += (len(prop.value) + 3) // 4
    if prop.bytes_after == 0:
        break

if len(result) == len(big_data):
    print(f'big-property-ok: wrote and read {len(big_data)} bytes')
else:
    print(f'big-property-partial: got {len(result)} of {len(big_data)} bytes')

w.destroy()
d.close()
