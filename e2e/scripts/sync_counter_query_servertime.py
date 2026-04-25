from Xlib import X, display
d = display.Display()
# Query the SYNC extension
ext = d.query_extension('SYNC')
print(f'SYNC ext present={ext is not None}')
d.close()
