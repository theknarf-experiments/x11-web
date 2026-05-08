"""Smoke check: the XFIXES extension is registered and has an opcode."""

import Xlib.display

d = Xlib.display.Display()
ext = d.query_extension("XFIXES")
if ext is not None and ext.major_opcode > 0:
    print(f"XFIXES_OK opcode={ext.major_opcode}")
else:
    print("XFIXES_NOT_FOUND")
