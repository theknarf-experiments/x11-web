from Xlib import display
d = display.Display()
ext = d.query_extension("RENDER")
print(f"render_present={ext is not None and ext.major_opcode > 0}")
d.close()
