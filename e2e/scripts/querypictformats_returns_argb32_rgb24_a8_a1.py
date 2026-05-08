from Xlib import display
d = display.Display()
ext = d.query_extension('RENDER')
print(f"render_present={bool(ext.present) if ext else False}")
d.close()
