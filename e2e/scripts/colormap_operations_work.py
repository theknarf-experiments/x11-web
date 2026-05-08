import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Allocate a named color from the default colormap
cmap = screen.default_colormap
color = cmap.alloc_named_color('red')
print(f"red_pixel={color.pixel}")
print(f"exact_red={color.exact_red}")

# Query the color back
qc = cmap.query_colors([color.pixel])
print(f"query_count={len(qc)}")

d.close()
