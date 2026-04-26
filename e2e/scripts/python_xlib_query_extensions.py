from Xlib import display
d = display.Display()
# python-xlib's list_extensions() returns plain strings, not objects
# with a .name attribute.
ext_names = list(d.list_extensions())
print(f"extensions: {len(ext_names)}")
for name in sorted(ext_names):
    print(f"  {name}")
assert "RANDR" in ext_names, "RANDR missing"
print("EXTENSIONS_OK")
d.close()
