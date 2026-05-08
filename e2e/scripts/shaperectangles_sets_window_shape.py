import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
shape = d.query_extension('SHAPE')
print(f"shape_present={shape is not None and shape.major_opcode > 0}")
d.close()
