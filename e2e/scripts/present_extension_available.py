import Xlib.display
d = Xlib.display.Display(":99")
ext = d.query_extension("Present")
assert ext is not None and ext.major_opcode > 0, "Present not found"
print(f"PRESENT_OPCODE={ext.major_opcode}")
print("PRESENT_OK")
d.close()
