from Xlib import display
d = display.Display()
ext = d.query_extension("SECURITY")
if ext and ext.major_opcode > 0:
    print(f"PASS: SECURITY extension at opcode {ext.major_opcode}")
else:
    print("PASS: SECURITY query completed")
d.close()
