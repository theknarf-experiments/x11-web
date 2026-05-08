import Xlib.display
d = Xlib.display.Display()
# Create a custom atom
atom = d.intern_atom('_TEST_CUSTOM_ATOM_12345')
name = d.get_atom_name(atom)
print(f"atom_id={atom} name={name}")
if name == '_TEST_CUSTOM_ATOM_12345':
    print("ATOM_OK")
# Verify only_if_exists works
atom2 = d.intern_atom('_NONEXISTENT_ATOM_ZZZZZ', only_if_exists=True)
print(f"nonexistent={atom2}")
if atom2 == 0:
    print("ONLY_IF_EXISTS_OK")
d.close()
