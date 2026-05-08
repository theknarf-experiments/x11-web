import Xlib.display
d = Xlib.display.Display()
# Intern a custom atom
atom_id = d.intern_atom('_X11WEB_TEST_ATOM', False)
# Get the name back
name = d.get_atom_name(atom_id)
print(f"atom_id={atom_id} name={name}")
d.close()
