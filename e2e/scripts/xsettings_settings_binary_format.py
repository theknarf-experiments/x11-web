import Xlib, Xlib.display, struct
d = Xlib.display.Display()
settings_atom = d.intern_atom('_XSETTINGS_SETTINGS')
s0_atom = d.intern_atom('_XSETTINGS_S0')
owner = d.get_selection_owner(s0_atom)
if not owner or owner.id == 0:
    print('no-owner')
    exit(0)
prop = owner.get_full_property(settings_atom, 0)
if not prop:
    print('no-property')
    exit(0)
data = bytes(prop.value)
if len(data) < 12:
    print(f'too-short: {len(data)}')
    exit(0)
byte_order = data[0]
serial = struct.unpack_from('<I' if byte_order == 0 else '>I', data, 4)[0]
n_settings = struct.unpack_from('<I' if byte_order == 0 else '>I', data, 8)[0]
print(f'xsettings-byte-order: {byte_order}')
print(f'xsettings-serial: {serial}')
print(f'xsettings-count: {n_settings}')
if n_settings >= 10: print('xsettings-format-ok')
