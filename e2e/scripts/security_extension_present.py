"""Smoke check: the SECURITY extension is registered and has an opcode."""

import Xlib.display

d = Xlib.display.Display()
ext = d.query_extension("SECURITY")
if ext is not None and ext.major_opcode > 0:
    print(f"SECURITY_OK opcode={ext.major_opcode}")
else:
    print("SECURITY_NOT_FOUND")
