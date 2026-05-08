import Xlib.display
d = Xlib.display.Display()

# Open a well-known font
font = d.open_font('fixed')
info = font.query()
print(f"min_bounds_width={info.min_bounds.character_width}")
print(f"max_bounds_width={info.max_bounds.character_width}")
print(f"ascent={info.font_ascent}")
print(f"descent={info.font_descent}")
print(f"font_ok={info.font_ascent > 0}")

font.close()
d.close()
