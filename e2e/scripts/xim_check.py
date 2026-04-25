import Xlib.display
import Xlib.X
import sys

d = Xlib.display.Display(':99')
root = d.screen().root

xim_servers_atom = d.intern_atom('_XIM_SERVERS', only_if_exists=True)
if xim_servers_atom == 0:
    # Atom not interned yet — server may not set it until a client asks
    xim_servers_atom = d.intern_atom('_XIM_SERVERS', only_if_exists=False)

prop = root.get_full_property(xim_servers_atom, Xlib.X.AnyPropertyType)
if prop is None:
    print("SKIP: _XIM_SERVERS property not set on root window")
    sys.exit(0)

# The property value is a list of atoms whose names are XIM server locators
# e.g. @server=x11web
atoms = prop.value
found = False
for atom_id in atoms:
    name = d.get_atom_name(atom_id)
    print(f"XIM_SERVER: {name}")
    if 'x11web' in name:
        found = True

if found:
    print("XIM_PASS")
else:
    print("XIM_WARN: x11web server not found in _XIM_SERVERS, but atom exists")

d.close()
