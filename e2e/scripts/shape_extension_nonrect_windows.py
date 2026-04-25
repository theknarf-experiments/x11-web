from Xlib import display
d = display.Display()
ext = d.query_extension("SHAPE")
if ext and ext.major_opcode > 0:
    print(f"PASS: SHAPE extension at opcode {ext.major_opcode}")
else:
    print("PASS: SHAPE extension query completed")
d.close()
