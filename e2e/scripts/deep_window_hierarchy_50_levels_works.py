from Xlib import X, display
d = display.Display()
root = d.screen().root

# Create 50-level deep hierarchy
depth = 50
current = root
windows = []
for i in range(depth):
    w = current.create_window(1, 1, 10, 10, 0, d.screen().root_depth)
    windows.append(w)
    current = w
d.sync()

# Query the deepest window's geometry
geom = windows[-1].get_geometry()
print(f"deepest_width={geom.width}")

# TranslateCoordinates from deepest to root
tc = windows[-1].translate_coords(root, 0, 0)
print(f"translate_x={tc.x} translate_y={tc.y}")

# Cleanup
windows[0].destroy()
d.close()
