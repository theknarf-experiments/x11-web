import Xlib.display
d = Xlib.display.Display()
# XInputExtension should be present
ext = d.query_extension('XInputExtension')
print(f"present={bool(ext is not None and ext.present)}")
d.close()
