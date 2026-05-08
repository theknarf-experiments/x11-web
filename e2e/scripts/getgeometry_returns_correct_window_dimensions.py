from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
w = root.create_window(50, 75, 320, 240, 2, screen.root_depth)
d.sync()
geo = w.get_geometry()
print(f"x={geo.x} y={geo.y} w={geo.width} h={geo.height} bw={geo.border_width}")
w.destroy()
d.close()
