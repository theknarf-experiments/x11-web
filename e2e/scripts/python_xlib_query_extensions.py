from Xlib import display
d = display.Display()
exts = d.list_extensions()
ext_names = [e.name for e in exts]
print(f'extensions: {len(ext_names)}')
for name in sorted(ext_names): print(f'  {name}')
assert b'RANDR' in ext_names or 'RANDR' in [n.decode() if isinstance(n, bytes) else n for n in ext_names], 'RANDR missing'
print('EXTENSIONS_OK')
d.close()
