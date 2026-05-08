"""
Walks the well-known X11 extensions, grabs each one's `first_event`
base, and asserts there's no overlap. Two extensions sharing an
event base would cause delivered events to be misrouted.
"""

import Xlib.display

d = Xlib.display.Display()

extensions = [
    "SHAPE",
    "SYNC",
    "RANDR",
    "XKEYBOARD",
    "DAMAGE",
    "MIT-SCREEN-SAVER",
    "XInputExtension",
    "XFIXES",
]

event_bases = {}
for name in extensions:
    ext = d.query_extension(name)
    if ext and ext.major_opcode > 0 and ext.first_event > 0:
        event_bases[name] = ext.first_event

values = list(event_bases.values())
unique = len(set(values)) == len(values)
if unique:
    print("EVENT_BASES_UNIQUE_OK")
else:
    print(f"EVENT_BASES_OVERLAP: {event_bases}")

for name, base in sorted(event_bases.items(), key=lambda x: x[1]):
    print(f"  {name}: event_base={base}")
