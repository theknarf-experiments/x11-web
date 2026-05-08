from Xlib import display
d = display.Display()
ext = d.query_extension('SYNC')
print(f"sync_present={bool(ext.present) if ext else False}")
d.close()
