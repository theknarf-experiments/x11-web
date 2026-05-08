import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

cmap = screen.default_colormap

# Alloc a known color
result = cmap.alloc_color(65535, 0, 0)  # Pure red
pixel = result.pixel

# Query the color back
colors = cmap.query_colors([pixel])
if colors:
    c = colors[0]
    print(f"red={c.red}")
    print(f"green={c.green}")
    print(f"blue={c.blue}")
    print(f"is_red={c.red == 65535 and c.green == 0 and c.blue == 0}")

d.close()
