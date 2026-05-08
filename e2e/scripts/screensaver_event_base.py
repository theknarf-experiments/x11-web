"""
The MIT-SCREEN-SAVER extension is expected to be present and to
report a non-zero `first_event` (the standard X11 base for
ScreenSaverNotify is 92 across most servers, but anything > 0 is
spec-conformant).
"""

import Xlib.display

d = Xlib.display.Display()
ext = d.query_extension("MIT-SCREEN-SAVER")

if ext is None:
    print("EXT_NOT_FOUND")
elif ext.major_opcode > 0:
    event_base = ext.first_event
    print(f"EXT_OK opcode={ext.major_opcode} event_base={event_base}")
    if event_base == 92:
        print("EVENT_BASE_92_OK")
    elif event_base > 0:
        print(f"EVENT_BASE_NONZERO_OK={event_base}")
    else:
        print("EVENT_BASE_ZERO_FAIL")
else:
    print("EXT_NO_OPCODE")
