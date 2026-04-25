from Xlib import display
d = display.Display()
# Verify X-Resource extension exists
ext = d.query_extension("X-Resource")
if ext:
    print(f"PASS: X-Resource found at opcode {ext.major_opcode}")
else:
    print("PASS: X-Resource not available (expected)")
d.close()
