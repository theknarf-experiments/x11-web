import Xlib.display
d = Xlib.display.Display()
xv = d.query_extension('XVideo')
print(f"xvideo_present={xv is not None and xv.major_opcode > 0}")
d.close()
