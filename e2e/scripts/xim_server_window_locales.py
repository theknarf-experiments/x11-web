import Xlib, Xlib.display
d = Xlib.display.Display()
root = d.screen().root
xim_atom = d.intern_atom('XIM_SERVERS')
prop = root.get_full_property(xim_atom, Xlib.X.AnyPropertyType)
if prop:
    print(f'xim-servers-property-type: {prop.property_type}')
    print('xim-server-found')
else:
    print('xim-no-servers')
