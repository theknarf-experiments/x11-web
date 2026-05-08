import Xlib.display
d = Xlib.display.Display()
fonts = d.list_fonts('*', 100)
has_fixed = 'fixed' in fonts
has_cursor = 'cursor' in fonts
print(f"has_fixed={has_fixed}")
print(f"has_cursor={has_cursor}")
print(f"total_fonts={len(fonts)}")
d.close()
