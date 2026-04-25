import Xlib, Xlib.display
d = Xlib.display.Display()
atom = d.intern_atom('_XSETTINGS_S0')
owner = d.get_selection_owner(atom)
print(f'xsettings-owner: {owner.id}' if owner else 'xsettings-owner: none')
if owner and owner.id != 0: print('xsettings-owner-ok')
