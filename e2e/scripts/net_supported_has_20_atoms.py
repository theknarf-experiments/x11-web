import Xlib.display
d = Xlib.display.Display()
net_supported = d.intern_atom('_NET_SUPPORTED')
prop = d.screen().root.get_full_property(net_supported, 0)
print(f"count={len(prop.value) if prop else 0}")
d.close()
