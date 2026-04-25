import Xlib.display, sys
d = Xlib.display.Display()
ext = d.query_extension("RECORD")
if ext and ext.present:
    print(f"record-ok: opcode={ext.major_opcode}")
else:
    print("record-fail: extension not present")
d.close()
