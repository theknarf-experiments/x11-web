from Xlib import display
d = display.Display()
# Query keycodes 9-65 (Escape through Space)
mapping = d.get_keyboard_mapping(9, 57)
# Keycode 9 = Escape (0xff1b)
esc = mapping[0][0]
# Keycode 36 = Return (0xff0d)
ret = mapping[27][0]
# Keycode 65 = Space (0x0020)
space = mapping[56][0]
print(f"escape={hex(esc)} return={hex(ret)} space={hex(space)}")
d.close()
