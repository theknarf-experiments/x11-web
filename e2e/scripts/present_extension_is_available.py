import Xlib.display
d = Xlib.display.Display()
present = d.query_extension('Present')
print(f"present_present={present is not None and present.major_opcode > 0}")
d.close()
