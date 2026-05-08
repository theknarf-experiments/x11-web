import Xlib.display
d = Xlib.display.Display()

# Query _NET_SYSTEM_TRAY_S0 selection owner
tray_atom = d.intern_atom('_NET_SYSTEM_TRAY_S0')
owner = d.get_selection_owner(tray_atom)

if owner and owner.id != 0:
    print(f"result=OK,owner={owner.id:#x}")
else:
    print("result=NO_OWNER")

d.close()
