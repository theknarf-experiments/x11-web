from Xlib import display
d = display.Display()
ext = d.query_extension('MIT-SHM')
print(f"shm_present={bool(ext.present) if ext else False}")
d.close()
