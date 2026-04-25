from Xlib import display
d = display.Display()
ext = d.query_extension("XFIXES")
if ext and ext.present:
    print(f"PASS: XFIXES extension present (opcode={ext.major_opcode})")
else:
    print("FAIL: XFIXES extension not present")
d.close()
