from Xlib import display
d = display.Display()
# Query all extensions
extensions = d.list_extensions()
print(f"Total extensions: {len(extensions)}")
# Verify key extensions are present
ext_names = set(extensions)
required = {"RENDER", "RANDR", "XFIXES", "SHAPE", "SYNC", "XInputExtension"}
missing = required - ext_names
if not missing:
    print(f"PASS: all {len(required)} required extensions present, {len(extensions)} total")
else:
    print(f"FAIL: missing extensions: {missing}")
d.close()
