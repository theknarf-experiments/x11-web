import Xlib.display, Xlib.X, sys, threading
passed = 0; failed = 0; errors = []

# Open 5 independent connections
connections = []
for i in range(5):
    try:
        d = Xlib.display.Display()
        connections.append(d)
    except Exception as e:
        errors.append(f"connect {i}: {e}")

if len(connections) == 5:
    passed += 1; print("PASS: 5 concurrent connections")
else:
    failed += 1; print(f"FAIL: only {len(connections)} connections")

# Each connection creates and queries its own window
windows = []
for i, d in enumerate(connections):
    root = d.screen().root
    w = root.create_window(i*50, 0, 100, 100, 0,
        d.screen().root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent)
    w.map()
    d.sync()
    windows.append(w)

# Verify each connection can see its own window
all_ok = True
for i, (d, w) in enumerate(zip(connections, windows)):
    try:
        attrs = w.get_attributes()
        if attrs.map_state != Xlib.X.IsViewable:
            all_ok = False; errors.append(f"window {i} not viewable")
    except Exception as e:
        all_ok = False; errors.append(f"query window {i}: {e}")

if all_ok:
    passed += 1; print("PASS: all windows viewable")
else:
    failed += 1; print(f"FAIL: {errors}")

# Clean up
for w in windows: w.destroy()
for d in connections:
    d.sync()
    d.close()

print(f"multi-client: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
