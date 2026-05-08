from Xlib import display
d = display.Display()
ext = d.query_extension('XKEYBOARD')
print(f"present={bool(ext.present) if ext else False}")
d.close()
