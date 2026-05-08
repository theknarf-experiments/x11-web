import Xlib.display
d = Xlib.display.Display()
screen = d.screen()

geo = screen.root.get_geometry()
print(f"root_width={geo.width}")
print(f"root_height={geo.height}")
print(f"root_depth={geo.depth}")
print(f"valid={geo.width > 0 and geo.height > 0 and geo.depth > 0}")

d.close()
