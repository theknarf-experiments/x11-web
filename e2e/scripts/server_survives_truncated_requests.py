import Xlib.display, Xlib.X, sys
# Connect and verify server is alive
d = Xlib.display.Display()
root = d.screen().root
# Create and destroy windows rapidly with edge-case sizes
for i in range(50):
    w = root.create_window(0, 0, max(1, i % 4), max(1, i % 3), 0, d.screen().root_depth)
    w.map()
    d.sync()
    w.unmap()
    w.destroy()
    d.sync()
# Verify server still responds
g = root.get_geometry()
assert g.width > 0
d.close()
# Reconnect to verify server is stable
d2 = Xlib.display.Display()
g2 = d2.screen().root.get_geometry()
assert g2.width > 0
d2.close()
print("fuzz-survive-ok")
