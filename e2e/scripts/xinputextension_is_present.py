from Xlib import display
d = display.Display()
ext = d.query_extension('XInputExtension')
print(f"xi_present={bool(ext.present) if ext else False}")
d.close()
