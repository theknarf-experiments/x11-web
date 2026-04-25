import Xlib.display
d = Xlib.display.Display(":99")
# Query XFIXES extension
ext = d.query_extension("XFIXES")
assert ext is not None, "XFIXES not found"
assert ext.major_opcode > 0, f"XFIXES has no opcode"
print(f"XFIXES_OPCODE={ext.major_opcode}")
print("XFIXES_OK")
d.close()
