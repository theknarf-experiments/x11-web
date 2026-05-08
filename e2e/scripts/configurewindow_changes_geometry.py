import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(10, 10, 100, 100, 0, screen.root_depth)
w.map()
d.sync()

# Resize
w.configure(width=200, height=150)
d.sync()

geo = w.get_geometry()
print(f"width={geo.width} height={geo.height}")

w.destroy()
d.close()
