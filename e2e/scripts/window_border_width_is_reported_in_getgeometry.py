import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with border_width=5
w = screen.root.create_window(10, 20, 100, 50, 5, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    border_pixel=0xFF0000)
w.map()
d.sync()

geo = w.get_geometry()
print(f"border_width={geo.border_width}")
print(f"width={geo.width}")
print(f"height={geo.height}")

# Change border width
w.configure(border_width=10)
d.sync()
geo2 = w.get_geometry()
print(f"new_border_width={geo2.border_width}")

w.destroy()
d.close()
