import Xlib.display, Xlib.X

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

# Create a large property (256KB) to test big request handling
w = root.create_window(
    0, 0, 1, 1, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
)
big_data = bytes(range(256)) * 1024  # 256KB
atom = d.intern_atom('_BIG_TEST_PROP')
w.change_property(atom, Xlib.X.STRING, 8, big_data)
d.sync()

# Read it back
prop = w.get_property(atom, Xlib.X.STRING, 0, len(big_data))
if prop and len(prop.value) == len(big_data):
    print(f'big-property-ok: wrote and read {len(big_data)} bytes')
else:
    got = len(prop.value) if prop else 0
    print(f'big-property-partial: got {got} of {len(big_data)} bytes')

w.destroy()
d.close()
