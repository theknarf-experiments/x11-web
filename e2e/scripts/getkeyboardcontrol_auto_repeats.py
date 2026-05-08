"""
`GetKeyboardControl` should return `auto_repeats` as a 32-byte bitmap
(one bit per keycode in 0..255). Initially every key auto-repeats,
so all bytes are 0xFF; we accept partial bitmaps to allow servers
that exclude some modifier keys by default.
"""

import Xlib.display

d = Xlib.display.Display()
ctrl = d.get_keyboard_control()
ar = ctrl.auto_repeats

if len(ar) == 32:
    all_ff = all(b == 0xFF for b in ar)
    if all_ff:
        print("AUTO_REPEATS_ALL_ON")
    else:
        print(f"AUTO_REPEATS_PARTIAL: {[hex(b) for b in ar[:8]]}")
else:
    print(f"AUTO_REPEATS_WRONG_LEN: {len(ar)}")
