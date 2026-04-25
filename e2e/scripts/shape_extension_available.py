import Xlib.display
d = Xlib.display.Display(":99")
ext = d.query_extension("SHAPE")
assert ext is not None and ext.major_opcode > 0, "SHAPE not found"
print(f"SHAPE_OPCODE={ext.major_opcode}")
print("SHAPE_OK")
d.close()
