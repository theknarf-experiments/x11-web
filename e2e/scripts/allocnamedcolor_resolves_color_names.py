import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
cm = screen.default_colormap

color = cm.alloc_named_color('red')
print(f"red_pixel={color.pixel}")
print(f"red_exact_r={color.exact_red}")

color2 = cm.alloc_named_color('blue')
print(f"blue_pixel={color2.pixel}")
print(f"blue_exact_b={color2.exact_blue}")

# Named colors should resolve to proper RGB
print(f"red_ok={color.exact_red == 0xFFFF and color.exact_green == 0 and color.exact_blue == 0}")
print(f"blue_ok={color2.exact_red == 0 and color2.exact_green == 0 and color2.exact_blue == 0xFFFF}")

d.close()
