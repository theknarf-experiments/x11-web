import Xlib.display
d = Xlib.display.Display()
xkb = d.query_extension('XKEYBOARD')
print(f"present={xkb is not None and xkb.major_opcode > 0}")
if xkb:
    print(f"major_opcode={xkb.major_opcode}")
    print(f"first_event={xkb.first_event}")
    print(f"has_event_base={xkb.first_event > 0}")
d.close()
