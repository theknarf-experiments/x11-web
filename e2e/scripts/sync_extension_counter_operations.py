import Xlib.display, Xlib.X

d = Xlib.display.Display()

# Check SYNC extension
sync_info = d.query_extension('SYNC')
if sync_info and sync_info.present:
    print("SYNC_PRESENT")
else:
    print("SYNC_MISSING")

d.close()
