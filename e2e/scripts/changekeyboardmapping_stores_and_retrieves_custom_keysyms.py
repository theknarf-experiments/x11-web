from Xlib import X, display
d = display.Display()
# Get current mapping for keycode 38 (normally 'a')
mapping = d.get_keyboard_mapping(38, 1)
orig = mapping[0][0]
print(f"original_keysym={hex(orig)}")
# Change keycode 38 to produce 'z' (0x7a) / 'Z' (0x5a)
d.change_keyboard_mapping(38, [(0x7a, 0x5a, 0, 0)])
d.sync()
# Verify it took effect
mapping2 = d.get_keyboard_mapping(38, 1)
new_sym = mapping2[0][0]
print(f"new_keysym={hex(new_sym)}")
# Restore original
d.change_keyboard_mapping(38, [(orig, mapping[0][1] if len(mapping[0]) > 1 else orig, 0, 0)])
d.sync()
d.close()
