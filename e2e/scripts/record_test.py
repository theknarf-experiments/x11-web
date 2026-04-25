import Xlib.display
import sys

d = Xlib.display.Display(':99')
ext = d.query_extension('RECORD')
if ext is None or not ext.present:
    print("FAIL: RECORD not present")
    sys.exit(1)
print(f"PASS: RECORD present, major_opcode={ext.major_opcode}")
d.close()
print("RECORD_OK")
