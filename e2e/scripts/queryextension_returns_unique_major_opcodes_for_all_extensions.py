import Xlib.display
d = Xlib.display.Display()
extensions = [
    'BIG-REQUESTS', 'MIT-SHM', 'RENDER', 'XFIXES', 'SHAPE', 'SYNC',
    'Composite', 'DAMAGE', 'Present', 'RANDR', 'XKEYBOARD',
    'XTEST', 'DPMS', 'RECORD', 'SECURITY', 'XVideo',
    'DOUBLE-BUFFER', 'XINERAMA', 'GLX', 'X-Resource',
]
opcodes = {}
conflicts = []
for name in extensions:
    info = d.query_extension(name)
    if info and info.major_opcode > 0:
        code = info.major_opcode
        if code in opcodes:
            conflicts.append(f"{name}={code} conflicts with {opcodes[code]}")
        opcodes[code] = name
if conflicts:
    print(f"CONFLICTS: {', '.join(conflicts)}")
else:
    print(f"OK: {len(opcodes)} extensions with unique opcodes")
d.close()
