from Xlib import X, display
# 50 rapid connect/disconnect cycles
for i in range(50):
    d = display.Display()
    root = d.screen().root
    # Create and immediately destroy resources
    w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
        X.InputOutput, X.CopyFromParent)
    pm = root.create_pixmap(10, 10, d.screen().root_depth)
    gc = w.create_gc()
    gc.free()
    pm.free()
    w.destroy()
    d.sync()
    d.close()
# Verify server is still responsive
d = display.Display()
# python-xlib's Display only exposes `info` via the inner _Display
vendor = d.display.info.vendor
print(f"PASS: server healthy after 50 cycles, vendor={vendor}")
d.close()
