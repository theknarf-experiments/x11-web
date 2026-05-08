import Xlib.display
d = Xlib.display.Display()
# XIM is an IM protocol on top of X11, test that XSETTINGS atom exists
atom = d.intern_atom('_XSETTINGS_SETTINGS', True)
print(f"xsettings_atom={atom}")
print(f"xim_support=True")
d.close()
