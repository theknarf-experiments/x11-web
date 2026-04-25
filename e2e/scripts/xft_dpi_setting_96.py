import Xlib, Xlib.display, struct
d = Xlib.display.Display()
settings_atom = d.intern_atom('_XSETTINGS_SETTINGS')
s0_atom = d.intern_atom('_XSETTINGS_S0')
owner = d.get_selection_owner(s0_atom)
if not owner or owner.id == 0: exit(1)
prop = owner.get_full_property(settings_atom, 0)
data = bytes(prop.value)
bo = '<' if data[0] == 0 else '>'
n = struct.unpack_from(bo + 'I', data, 8)[0]
off = 12
for i in range(n):
    if off + 4 > len(data): break
    typ = data[off]
    name_len = struct.unpack_from(bo + 'H', data, off + 2)[0]
    name_pad = (name_len + 3) & ~3
    name = data[off + 4:off + 4 + name_len].decode('ascii', errors='replace')
    val_off = off + 4 + name_pad + 4
    if typ == 0 and val_off + 4 <= len(data):
        val = struct.unpack_from(bo + 'I', data, val_off)[0]
        if name == 'Xft/DPI':
            print(f'xft-dpi: {val}')
            if val == 98304: print('xft-dpi-ok')
        off = val_off + 4
    elif typ == 1 and val_off + 4 <= len(data):
        slen = struct.unpack_from(bo + 'I', data, val_off)[0]
        off = val_off + 4 + ((slen + 3) & ~3)
    elif typ == 2:
        off = val_off + 8
    else:
        break
