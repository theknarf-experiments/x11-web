import Xlib.display
d = Xlib.display.Display()
sync_ext = d.query_extension('SYNC')
if sync_ext:
    print(f"sync_present=true")
    print(f"sync_opcode={sync_ext.major_opcode}")
else:
    print("sync_present=false")
d.close()
