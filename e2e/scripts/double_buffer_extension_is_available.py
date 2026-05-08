import Xlib.display
d = Xlib.display.Display()
dbe = d.query_extension('DOUBLE-BUFFER')
print(f"dbe_present={dbe is not None and dbe.major_opcode > 0}")
d.close()
