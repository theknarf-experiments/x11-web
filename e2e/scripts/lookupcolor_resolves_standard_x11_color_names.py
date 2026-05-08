import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
cmap = screen.default_colormap

# Test well-known color names
colors = ["red", "green", "blue", "white", "black", "yellow", "cyan", "magenta"]
for name in colors:
    try:
        result = cmap.lookup_color(name)
        # result is (exact_color, screen_color)
        print(f"LOOKUP_{name.upper()}_OK")
    except Exception as e:
        print(f"LOOKUP_{name.upper()}_FAIL:{e}")
