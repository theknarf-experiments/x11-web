import Xlib.display
d = Xlib.display.Display()
xkb = d.query_extension('XKEYBOARD')
print(f"xkb_present={xkb is not None and xkb.major_opcode > 0}")
d.close()
