import Xlib.display, Xlib.X, struct
d = Xlib.display.Display()
screen = d.screen()
xsettings_screen = d.intern_atom('_XSETTINGS_S0')
owner = d.get_selection_owner(xsettings_screen)
print(f"xsettings_owner={owner != 0}")
if owner:
    settings_atom = d.intern_atom('_XSETTINGS_SETTINGS')
    prop = owner.get_full_property(settings_atom, 0)
    if prop and len(prop.value) > 12:
        data = bytes(prop.value)
        n = struct.unpack_from('<I' if data[0] == 0 else '>I', data, 8)[0]
        print(f"xsettings_count={n}")
    else:
        print("xsettings_count=0")
else:
    print("xsettings_count=0")
d.close()
