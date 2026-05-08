from Xlib import display
d = display.Display()
# Intern a custom atom
atom = d.intern_atom('X11_WEB_TEST_ATOM_12345')
print(f"atom_id={atom}")
# Get the name back
name = d.get_atom_name(atom)
print(f"atom_name={name}")
# Verify built-in atoms
primary = d.intern_atom('PRIMARY')
print(f"primary_atom={primary}")
d.close()
