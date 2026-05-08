from Xlib import X, display
d = display.Display()
root = d.screen().root

# Create parent window
parent = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth,
    event_mask=X.SubstructureNotifyMask)
parent.map()
d.sync()

# Create child with SouthEast gravity
child = parent.create_window(50, 50, 30, 30, 0, d.screen().root_depth,
    window_class=X.InputOutput)
child.change_attributes(win_gravity=9)  # SouthEast
child.map()
d.sync()

# Get child position before resize
geom_before = child.get_geometry()
print(f"before_x={geom_before.x} before_y={geom_before.y}")

# Resize parent (larger)
parent.configure(width=300, height=300)
d.sync()

# Get child position after resize — should shift with SouthEast gravity
geom_after = child.get_geometry()
print(f"after_x={geom_after.x} after_y={geom_after.y}")

# SouthEast: child should move by the same delta as the size increase
dx = geom_after.x - geom_before.x
dy = geom_after.y - geom_before.y
print(f"dx={dx} dy={dy}")

parent.destroy()
d.close()
