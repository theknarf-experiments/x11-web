import Xlib.display
d = Xlib.display.Display()
sync = d.query_extension('SYNC')
print(f"sync_present={sync is not None and sync.major_opcode > 0}")
d.close()
