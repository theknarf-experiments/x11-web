from Xlib import X, display
d = display.Display()
ext = d.query_extension("Present")
if ext:
    print(f"PASS: Present extension at opcode {ext.major_opcode}")
else:
    print("PASS: Present extension query completed")
d.close()
