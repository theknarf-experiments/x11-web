from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Open the 'fixed' font
try:
    fid = d.open_font('fixed')
    print(f"font_opened=True")
    info = d.query_font(fid)
    print(f"min_char={info.min_char_or_byte2}")
    print(f"max_char={info.max_char_or_byte2}")
    d.close_font(fid)
except Exception as e:
    print(f"font_error={e}")
d.close()
