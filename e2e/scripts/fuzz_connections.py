import Xlib.display
import sys
import time

errors = []
NUM_CONNECTIONS = 50

# Test 1: Rapid open/close cycle
start = time.time()
for i in range(NUM_CONNECTIONS):
    try:
        d = Xlib.display.Display(':99')
        # Do a minimal operation to confirm the connection works
        _ = d.screen().root
        d.close()
    except Exception as e:
        errors.append(f"Connection {i} failed: {e}")
        break

elapsed = time.time() - start
print(f"PASS: {NUM_CONNECTIONS} rapid open/close cycles in {elapsed:.2f}s")

# Test 2: Multiple simultaneous connections
connections = []
try:
    for i in range(10):
        d = Xlib.display.Display(':99')
        connections.append(d)
    print(f"PASS: {len(connections)} simultaneous connections opened")

    # Use each connection
    for i, d in enumerate(connections):
        root = d.screen().root
        geom = root.get_geometry()
        if geom.width <= 0:
            errors.append(f"Connection {i}: bad root geometry")

    print("PASS: all simultaneous connections functional")
finally:
    for d in connections:
        try:
            d.close()
        except:
            pass

print("PASS: all simultaneous connections closed cleanly")

# Test 3: Verify server still works after stress
d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth)
w.map()
d.sync()
w.destroy()
d.sync()
d.close()
print("PASS: server fully functional after connection stress")

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("FUZZING_CONNECTIONS_OK")
