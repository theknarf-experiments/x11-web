from Xlib import display, X, Xatom
d = display.Display()
targets_atom = d.intern_atom('TARGETS')
utf8_atom = d.intern_atom('UTF8_STRING')
print(f"targets_atom={targets_atom}")
print(f"utf8_atom={utf8_atom}")
# Both atoms should be valid (non-zero)
print(f"atoms_valid={targets_atom > 0 and utf8_atom > 0}")
d.close()
