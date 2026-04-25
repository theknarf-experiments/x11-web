import Xlib.display
d = Xlib.display.Display(":99")
ext = d.query_extension("MIT-SHM")
assert ext is not None and ext.major_opcode > 0, "MIT-SHM not found"
print(f"SHM_OPCODE={ext.major_opcode}")
print("SHM_OK")
d.close()
