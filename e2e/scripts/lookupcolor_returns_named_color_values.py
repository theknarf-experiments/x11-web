import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
cmap = screen.default_colormap

# Look up "red"
try:
    result = cmap.lookup_color("red")
    print(f"exact_red={result.exact_red}")
    print(f"exact_green={result.exact_green}")
    print(f"exact_blue={result.exact_blue}")
    print(f"is_red={result.exact_red == 65535 and result.exact_green == 0 and result.exact_blue == 0}")
except Exception as e:
    print(f"error={e}")

d.close()
