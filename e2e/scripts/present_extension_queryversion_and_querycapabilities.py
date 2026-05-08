import Xlib.display
d = Xlib.display.Display()
# Query Present extension
ext = d.query_extension("Present")
if ext and ext.major_opcode > 0:
    print(f"present_opcode={ext.major_opcode}")
else:
    print("present=missing")
# Query XC-MISC extension
xcmisc = d.query_extension("XC-MISC")
if xcmisc and xcmisc.major_opcode > 0:
    print(f"xcmisc_opcode={xcmisc.major_opcode}")
else:
    print("xcmisc=missing")
d.close()
