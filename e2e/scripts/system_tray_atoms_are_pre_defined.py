import Xlib.display
d = Xlib.display.Display()
tray_opcode = d.intern_atom('_NET_SYSTEM_TRAY_OPCODE', True)
tray_s0 = d.intern_atom('_NET_SYSTEM_TRAY_S0', True)
print(f"tray_opcode_exists={tray_opcode > 0}")
print(f"tray_s0_exists={tray_s0 > 0}")
d.close()
