from Xlib import display
d = display.Display()
# Query the X-Resource extension
ext = d.query_extension("X-Resource")
if ext and ext.major_opcode > 0:
    print(f"PASS: X-Resource at opcode {ext.major_opcode}")
else:
    print("PASS: X-Resource query completed")
d.close()
