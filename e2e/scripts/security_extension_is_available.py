import Xlib.display
d = Xlib.display.Display()
sec = d.query_extension('SECURITY')
print(f"security_present={sec is not None and sec.major_opcode > 0}")
d.close()
