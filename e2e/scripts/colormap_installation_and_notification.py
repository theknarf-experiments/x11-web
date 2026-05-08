import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# The default colormap should be installed
cmaps = screen.root.list_installed_colormaps()
print(f"installed_count={len(cmaps)}")
print(f"has_default={screen.default_colormap.id in [c.id for c in cmaps]}")

# Create a new colormap
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth)
new_cmap = w.create_colormap(screen.root_visual, Xlib.X.AllocNone)
d.sync()

# Alloc some colors
red = new_cmap.alloc_color(65535, 0, 0)
green = new_cmap.alloc_color(0, 65535, 0)
blue = new_cmap.alloc_color(0, 0, 65535)

print(f"red_pixel={red.pixel}")
print(f"green_pixel={green.pixel}")
print(f"blue_pixel={blue.pixel}")
print(f"colors_allocated={red.pixel != 0 or green.pixel != 0}")

new_cmap.free()
w.destroy()
d.close()
