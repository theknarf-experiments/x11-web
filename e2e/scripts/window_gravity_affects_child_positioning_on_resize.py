import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)

# Create child with SouthEast gravity (9)
child = parent.create_window(50, 50, 30, 30, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    win_gravity=9)  # SouthEast
child.map()
parent.map()
d.sync()

geo1 = child.get_geometry()
print(f"before_x={geo1.x} before_y={geo1.y}")

# Resize parent - child should move with SouthEast gravity
parent.configure(width=300, height=300)
d.sync()

geo2 = child.get_geometry()
print(f"after_x={geo2.x} after_y={geo2.y}")

# With SouthEast gravity, when parent grows by (100,100),
# child should move by (100,100) to stay relative to bottom-right
expected_x = 50 + 100
expected_y = 50 + 100
print(f"gravity_correct={geo2.x == expected_x and geo2.y == expected_y}")

child.destroy()
parent.destroy()
d.close()
