import Xlib.display
import Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Test default colormap operations
cmap = screen.default_colormap

# AllocColor for pure red
color = cmap.alloc_color(65535, 0, 0)
print(f"alloc_pixel={color.pixel}")
print(f"alloc_red={color.red}")
print(f"alloc_green={color.green}")
print(f"alloc_blue={color.blue}")

# QueryColor
qc = cmap.query_colors([color.pixel])
if qc:
    print(f"query_red={qc[0].red}")

# AllocNamedColor
try:
    named = cmap.alloc_named_color('blue')
    print(f"named_blue_pixel={named.pixel}")
    print("named_alloc=ok")
except:
    print("named_alloc=failed")

print("colormap_test=ok")
d.close()
