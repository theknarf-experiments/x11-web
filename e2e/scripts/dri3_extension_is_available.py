import Xlib.display
d = Xlib.display.Display()
dri3 = d.query_extension('DRI3')
print(f"dri3_present={dri3 is not None and dri3.major_opcode > 0}")
d.close()
