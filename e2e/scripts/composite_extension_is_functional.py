import Xlib.display
d = Xlib.display.Display()
comp = d.query_extension('Composite')
print(f"composite_present={comp is not None and comp.major_opcode > 0}")
d.close()
