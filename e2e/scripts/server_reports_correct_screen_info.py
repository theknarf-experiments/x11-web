import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
print(f"width={screen.width_in_pixels}")
print(f"height={screen.height_in_pixels}")
print(f"depth={screen.root_depth}")
print(f"screens={d.screen_count()}")
d.close()
