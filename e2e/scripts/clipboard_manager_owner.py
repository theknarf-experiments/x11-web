import Xlib, Xlib.display
d = Xlib.display.Display()
atom = d.intern_atom('CLIPBOARD_MANAGER')
owner = d.get_selection_owner(atom)
print(f'clipboard-mgr-owner: {owner.id}' if owner else 'clipboard-mgr-owner: none')
if owner and owner.id != 0: print('clipboard-mgr-ok')
