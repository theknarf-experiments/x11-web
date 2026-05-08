from Xlib import display
d = display.Display()
ext = d.query_extension('Composite')
print(f"composite_present={bool(ext.present) if ext else False}")
d.close()
