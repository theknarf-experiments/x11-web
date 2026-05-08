import Xlib.display
d = Xlib.display.Display()
# Open a font using XLFD pattern with wildcards
try:
    font = d.open_font('-*-fixed-*-*-*-*-*-*-*-*-*-*-*-*')
    qi = font.query()
    print(f"font_ascent={qi.font_ascent} font_descent={qi.font_descent}")
    print(f"min_char={qi.min_char_or_byte2} max_char={qi.max_char_or_byte2}")
    print("FONT_OK")
    font.close()
except Exception as e:
    print(f"ERROR: {e}")
d.close()
