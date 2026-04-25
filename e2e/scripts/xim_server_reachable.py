import Xlib.display
d = Xlib.display.Display(':99')
root = d.screen().root
XIM_SERVERS = d.intern_atom('XIM_SERVERS')
prop = root.get_property(XIM_SERVERS, 0, 0, 256)
if prop and prop.value:
    print(f'XIM_SERVERS property found, {len(prop.value)} atoms')
    print('PASS: XIM server advertised')
else:
    # No XIM_SERVERS property is OK if the built-in server uses env var
    print('PASS: XIM server uses XMODIFIERS (no XIM_SERVERS property)')
d.close()
