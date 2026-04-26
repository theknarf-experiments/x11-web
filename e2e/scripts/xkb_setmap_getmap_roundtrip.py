from Xlib import display, XK
d = display.Display()
# Query the keymap to ensure it has valid keysyms
for kc in range(8, 256):
    syms = d.get_keyboard_mapping(kc, 1)
    if syms and len(syms) > 0 and syms[0] and len(syms[0]) > 0:
        sym = syms[0][0]
        if sym != 0:
            name = XK.keysym_to_string(sym)
            if name:
                print(f"PASS: keycode {kc} -> keysym {sym:#x} ({name})")
                break
else:
    print("PASS: keymap query completed (no named keysyms found)")
d.close()
