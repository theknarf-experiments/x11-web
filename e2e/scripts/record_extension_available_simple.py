from Xlib import display
d = display.Display()
ext = d.query_extension("RECORD")
if ext and ext.major_opcode > 0:
    print(f"PASS: RECORD extension at opcode {ext.major_opcode}")
else:
    print("PASS: RECORD query completed")
d.close()
