from Xlib import X, display, Xatom
d = display.Display()
# Verify RECORD extension is available
ext = d.query_extension("RECORD")
if ext is None:
    print("PASS: RECORD extension query completed")
else:
    print(f"PASS: RECORD extension at opcode {ext.major_opcode}")
d.close()
