import Xlib.display
d = Xlib.display.Display()
xi = d.query_extension('XInputExtension')
print(f"xi_present={xi is not None and xi.major_opcode > 0}")
if xi:
    print(f"major_opcode={xi.major_opcode}")
d.close()
