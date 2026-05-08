from Xlib import display
d = display.Display()
font = d.open_font('fixed')
# query_text_extents lives on the Font/Fontable, not Display, and
# wants a list of CARD16 codepoints, not a Python string.
extents = font.query_text_extents([ord(c) for c in 'Hello World'])
print(f"ascent={extents.font_ascent}")
print(f"descent={extents.font_descent}")
print(f"width={extents.overall_width}")
print(f"valid={extents.font_ascent > 0 and extents.overall_width > 0}")
font.close()
d.close()
