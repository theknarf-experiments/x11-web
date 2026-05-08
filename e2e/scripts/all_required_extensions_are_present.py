import Xlib.display
d = Xlib.display.Display()
for ext in ['MIT-SHM', 'RENDER', 'Composite', 'RANDR', 'XKEYBOARD',
            'SHAPE', 'SYNC', 'XFIXES', 'DAMAGE', 'XInputExtension',
            'XTEST', 'RECORD', 'SECURITY', 'BIG-REQUESTS', 'XC-MISC']:
    r = d.query_extension(ext)
    ok = r is not None and r.major_opcode > 0
    print(f"{ext}={ok}")
d.close()
