import Xlib.display
d = Xlib.display.Display()
fonts = d.list_fonts('*', 100)
print(f"font_count={len(fonts)}")
fixed = d.list_fonts('fixed', 10)
print(f"has_fixed={len(fixed) > 0}")
d.close()
