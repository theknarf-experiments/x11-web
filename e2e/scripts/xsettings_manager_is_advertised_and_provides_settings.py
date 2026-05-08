import Xlib.display
d = Xlib.display.Display()

# Query _XSETTINGS_S0 selection owner
xs_atom = d.intern_atom('_XSETTINGS_S0')
owner = d.get_selection_owner(xs_atom)

if owner and owner.id != 0:
    # Check that XSETTINGS_SETTINGS property exists on the owner window
    settings_atom = d.intern_atom('_XSETTINGS_SETTINGS')
    prop = owner.get_full_property(settings_atom, 0)
    if prop and len(prop.value) > 0:
        print(f"result=OK,owner={owner.id:#x},data_len={len(prop.value)}")
    else:
        print(f"result=NO_SETTINGS,owner={owner.id:#x}")
else:
    print("result=NO_OWNER")

d.close()
