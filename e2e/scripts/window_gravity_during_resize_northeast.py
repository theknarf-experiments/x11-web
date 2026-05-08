import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create parent and child with NorthEastGravity
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)
parent.map()
d.sync()

child = parent.create_window(
    100, 10, 50, 50, 0, screen.root_depth,
    win_gravity=Xlib.X.NorthEastGravity,
)
child.map()
d.sync()

geo_before = child.get_geometry()
x_before = geo_before.x

# Resize the parent wider by 40 pixels
parent.configure(width=240)
d.sync()

import time
time.sleep(0.2)

geo_after = child.get_geometry()
x_after = geo_after.x

# With NorthEast gravity, the child should shift right by 40
# (maintaining its distance from the right edge)
delta = x_after - x_before
print(f"x_before={x_before} x_after={x_after} delta={delta}")
print(f"gravity_correct={delta == 40}")

child.destroy()
parent.destroy()
d.close()
