import Xlib.display
d = Xlib.display.Display(":99")
ext = d.query_extension("SYNC")
assert ext is not None and ext.major_opcode > 0, "SYNC not found"
print(f"SYNC_OPCODE={ext.major_opcode}")
print("SYNC_OK")
d.close()
