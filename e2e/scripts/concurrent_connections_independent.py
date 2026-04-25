from Xlib import X, display
# Open 5 concurrent connections
connections = []
windows = []
for i in range(5):
    d = display.Display()
    root = d.screen().root
    w = root.create_window(i*10, i*10, 50, 50, 0,
        d.screen().root_depth, X.InputOutput, X.CopyFromParent)
    w.map()
    d.sync()
    connections.append(d)
    windows.append(w)
print(f"PASS: {len(connections)} concurrent connections created")
# Verify each connection can see its window
for i, (d, w) in enumerate(zip(connections, windows)):
    geom = w.get_geometry()
    assert geom.width == 50, f"connection {i} bad width"
print("PASS: all connections verified independently")
# Close in reverse order
for d in reversed(connections):
    d.close()
print("PASS: all connections closed cleanly")
