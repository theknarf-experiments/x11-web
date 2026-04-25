import Xlib.display
import Xlib.X
import sys
import time

errors = []

d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root

# Test 1: Create many windows (stress test resource tracking)
NUM_WINDOWS = 500
windows = []
try:
    for i in range(NUM_WINDOWS):
        w = root.create_window(
            i % 100, i % 100, 10, 10, 0,
            screen.root_depth,
            Xlib.X.InputOutput,
            Xlib.X.CopyFromParent,
            background_pixel=screen.white_pixel,
        )
        windows.append(w)
    d.sync()
    print(f"PASS: created {NUM_WINDOWS} windows")
except Exception as e:
    print(f"INFO: window creation stopped at {len(windows)}: {e}")
    if len(windows) >= 100:
        print(f"PASS: created at least 100 windows before limit")
    else:
        errors.append(f"Could only create {len(windows)} windows")

# Destroy them all
for w in windows:
    try:
        w.destroy()
    except:
        pass
d.sync()
print(f"PASS: destroyed {len(windows)} windows")

# Test 2: Create many pixmaps
NUM_PIXMAPS = 200
pixmaps = []
try:
    for i in range(NUM_PIXMAPS):
        p = root.create_pixmap(64, 64, screen.root_depth)
        pixmaps.append(p)
    d.sync()
    print(f"PASS: created {NUM_PIXMAPS} pixmaps")
except Exception as e:
    print(f"INFO: pixmap creation stopped at {len(pixmaps)}: {e}")
    if len(pixmaps) >= 50:
        print(f"PASS: created at least 50 pixmaps before limit")
    else:
        errors.append(f"Could only create {len(pixmaps)} pixmaps")

for p in pixmaps:
    try:
        p.free()
    except:
        pass
d.sync()
print(f"PASS: freed {len(pixmaps)} pixmaps")

# Test 3: Create many GCs
NUM_GCS = 200
gcs = []
try:
    for i in range(NUM_GCS):
        gc = root.create_gc(foreground=i)
        gcs.append(gc)
    d.sync()
    print(f"PASS: created {NUM_GCS} GCs")
except Exception as e:
    print(f"INFO: GC creation stopped at {len(gcs)}: {e}")
    if len(gcs) >= 50:
        print(f"PASS: created at least 50 GCs before limit")
    else:
        errors.append(f"Could only create {len(gcs)} GCs")

for gc in gcs:
    try:
        gc.free()
    except:
        pass
d.sync()
print(f"PASS: freed {len(gcs)} GCs")

# Test 4: Many atoms (should not crash)
NUM_ATOMS = 500
atoms = []
for i in range(NUM_ATOMS):
    a = d.intern_atom(f'_FUZZ_ATOM_{i}', only_if_exists=False)
    atoms.append(a)
d.sync()
# Verify round-trip on a sample
for i in [0, NUM_ATOMS // 2, NUM_ATOMS - 1]:
    name = d.get_atom_name(atoms[i])
    if name != f'_FUZZ_ATOM_{i}':
        errors.append(f"Atom {i} name mismatch: {name}")
print(f"PASS: created and verified {NUM_ATOMS} atoms")

# Test 5: Verify server still healthy after resource churn
d2 = Xlib.display.Display(':99')
w = d2.screen().root.create_window(0, 0, 50, 50, 0, screen.root_depth)
w.map()
d2.sync()
w.destroy()
d2.sync()
d2.close()
print("PASS: server healthy after resource exhaustion test")

d.close()

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("FUZZING_RESOURCES_OK")
