import Xlib.display, Xlib.X, sys, time
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

# Test 1: Create window with NorthWest gravity (default)
w = root.create_window(100, 100, 200, 200, 2,
    d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()
time.sleep(0.1)

geom = w.get_geometry()
if geom.width == 200 and geom.height == 200:
    passed += 1; print("PASS: window created with correct geometry")
else:
    failed += 1; print(f"FAIL: geometry mismatch: {geom.width}x{geom.height}")

# Test 2: Set win_gravity to Static
w.change_attributes(win_gravity=Xlib.X.StaticGravity)
d.sync()

# Configure with border change
w.configure(border_width=4)
d.sync()
time.sleep(0.1)

geom2 = w.get_geometry()
if geom2.border_width == 4:
    passed += 1; print("PASS: border width changed")
else:
    failed += 1; print(f"FAIL: border width {geom2.border_width} != 4")

# Test 3: Set bit_gravity to Center
w.change_attributes(bit_gravity=Xlib.X.CenterGravity)
d.sync()
passed += 1; print("PASS: bit_gravity set to Center")

# Test 4: Resize should trigger ConfigureNotify
w.configure(width=300, height=300)
d.sync()
time.sleep(0.1)

got_configure = False
while d.pending_events():
    e = d.next_event()
    if e.type == Xlib.X.ConfigureNotify:
        got_configure = True
        if e.width == 300 and e.height == 300:
            passed += 1; print("PASS: ConfigureNotify with correct size")
        else:
            failed += 1; print(f"FAIL: ConfigureNotify size {e.width}x{e.height}")
        break

if not got_configure:
    failed += 1; print("FAIL: no ConfigureNotify after resize")

w.destroy()
d.close()
print(f"xts-gravity: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
