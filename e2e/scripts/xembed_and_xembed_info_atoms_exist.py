import Xlib.display
d = Xlib.display.Display()
xembed = d.intern_atom('_XEMBED', True)
xembed_info = d.intern_atom('_XEMBED_INFO', True)
print(f"xembed_atom={xembed}")
print(f"xembed_info_atom={xembed_info}")
print(f"xembed_present={xembed > 0}")
print(f"xembed_info_present={xembed_info > 0}")
d.close()
