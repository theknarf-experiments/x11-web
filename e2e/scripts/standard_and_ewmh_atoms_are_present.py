import Xlib.display
d = Xlib.display.Display()
for name in ['PRIMARY', 'WM_NAME', 'WM_CLASS', '_NET_WM_STATE', '_NET_SUPPORTED']:
    atom = d.intern_atom(name, True)
    print(f"{name}={atom}")
d.close()
