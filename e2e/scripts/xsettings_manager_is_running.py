import Xlib.display
d = Xlib.display.Display()
screen = d.screen()

# Check for _XSETTINGS_S0 selection owner
xsettings_atom = d.intern_atom('_XSETTINGS_S0')
owner = d.get_selection_owner(xsettings_atom)
print(f"xsettings_owner={owner.id if owner else 0}")

d.close()
