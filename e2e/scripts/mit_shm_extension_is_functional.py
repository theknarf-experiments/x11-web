import Xlib.display
d = Xlib.display.Display()
shm = d.query_extension('MIT-SHM')
print(f"shm_present={shm is not None and shm.major_opcode > 0}")
d.close()
