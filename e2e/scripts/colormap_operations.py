import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create a colormap
cmap = screen.default_colormap
# AllocColor
result = cmap.alloc_color(65535, 0, 0)  # Red
print(f"alloc_red: pixel={result.pixel}")
if result.pixel > 0 or result.exact_red == 65535:
    print("ALLOC_COLOR_OK")

# AllocNamedColor
try:
    result2 = cmap.alloc_named_color('blue')
    print(f"alloc_blue: pixel={result2.pixel}")
    print("ALLOC_NAMED_OK")
except Exception as e:
    print(f"AllocNamedColor: {e}")

# QueryColors
colors = cmap.query_colors([result.pixel])
if len(colors) > 0:
    print("QUERY_COLORS_OK")

d.close()
