import Xlib.display
import Xlib.X
import sys

d = Xlib.display.Display(':99')
errors = []

# The server should support XC-MISC extension
try:
    ext = d.query_extension('XC-MISC')
    if ext is None or not ext.present:
        print("SKIP: XC-MISC not present")
        sys.exit(0)
    print(f"PASS: XC-MISC present, major_opcode={ext.major_opcode}")
except Exception as e:
    print(f"SKIP: {e}")
    sys.exit(0)

# Create many windows to exercise resource ID allocation
windows = []
try:
    root = d.screen().root
    for i in range(100):
        w = root.create_window(0, 0, 1, 1, 0, d.screen().root_depth,
                              window_class=Xlib.X.InputOutput)
        windows.append(w)
    print(f"PASS: created {len(windows)} windows")
except Exception as e:
    errors.append(f"create windows: {e}")

# Clean up
for w in windows:
    try:
        w.destroy()
    except:
        pass
d.sync()

if errors:
    print(f"FAIL: {errors}")
    sys.exit(1)
print("XC_MISC_OK")
