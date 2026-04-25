import Xlib.display, Xlib.X, threading, time
results = []
def client_work(idx):
    try:
        d = Xlib.display.Display()
        root = d.screen().root
        # Create window
        w = root.create_window(10*idx, 10*idx, 100, 100, 0,
            d.screen().root_depth, Xlib.X.InputOutput,
            Xlib.X.CopyFromParent)
        w.map()
        d.sync()
        # Set property
        w.change_property(d.intern_atom("TEST_PROP"), Xlib.X.AnyPropertyType, 8,
            f"client{idx}".encode())
        d.sync()
        # Read back
        prop = w.get_full_property(d.intern_atom("TEST_PROP"), Xlib.X.AnyPropertyType)
        assert prop is not None, f"Property missing for client {idx}"
        # Create pixmap
        pm = w.create_pixmap(50, 50, d.screen().root_depth)
        gc = root.create_gc(foreground=0xFF0000)
        pm.fill_rectangle(gc, 0, 0, 50, 50)
        w.copy_area(gc, pm, 0, 0, 50, 50, 0, 0)
        gc.free()
        pm.free()
        d.sync()
        # Destroy
        w.destroy()
        d.sync()
        d.close()
        results.append((idx, "PASS"))
    except Exception as e:
        results.append((idx, f"FAIL: {e}"))

threads = []
for i in range(10):
    t = threading.Thread(target=client_work, args=(i,))
    threads.append(t)
    t.start()
for t in threads:
    t.join(timeout=30)
passes = sum(1 for _, r in results if r == "PASS")
fails = [f"{i}: {r}" for i, r in results if r != "PASS"]
if fails:
    print(f"FAIL: {len(fails)} clients failed: " + "; ".join(fails))
else:
    print(f"PASS: all {passes} clients succeeded")
