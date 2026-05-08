import Xlib.display
d = Xlib.display.Display()
font = d.open_font('fixed')
qi = font.query()
# query_text_extents requires a list of char codes (16-bit ints), not a string
ext = font.query_text_extents([ord(c) for c in 'Hello World'])
print(f"overall_width={ext.overall_width}")
print(f"font_ascent={ext.font_ascent}")
print(f"font_descent={ext.font_descent}")
# Width must be positive and reasonable
if ext.overall_width > 0:
    print("EXTENTS_OK")
font.close()
d.close()
