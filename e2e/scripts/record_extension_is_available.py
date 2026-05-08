import Xlib.display
d = Xlib.display.Display()
rec = d.query_extension('RECORD')
print(f"record_present={rec is not None and rec.major_opcode > 0}")
d.close()
