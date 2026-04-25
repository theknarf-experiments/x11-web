from Xlib import display
d = display.Display()
# Verify Sync extension is available
ext = d.query_extension("SYNC")
if ext and ext.present:
    print(f"PASS: SYNC extension present (opcode={ext.major_opcode})")
else:
    print("FAIL: SYNC extension not present")
d.close()
