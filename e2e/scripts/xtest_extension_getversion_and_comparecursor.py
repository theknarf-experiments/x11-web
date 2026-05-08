import Xlib.display
d = Xlib.display.Display()
ext = d.query_extension("XTEST")
if ext and ext.major_opcode > 0:
    print(f"xtest_opcode={ext.major_opcode}")
else:
    print("xtest=missing")
d.close()
