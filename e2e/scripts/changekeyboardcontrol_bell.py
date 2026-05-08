"""
`ChangeKeyboardControl` with `bell_percent` and `bell_pitch` should
update the values reported by a subsequent `GetKeyboardControl`.
"""

import Xlib.display

d = Xlib.display.Display()

d.change_keyboard_control(bell_percent=75)
d.sync()
ctrl = d.get_keyboard_control()
if ctrl.bell_percent == 75:
    print("BELL_PERCENT_OK")
else:
    print(f"BELL_PERCENT_FAIL: got {ctrl.bell_percent}")

d.change_keyboard_control(bell_pitch=800)
d.sync()
ctrl2 = d.get_keyboard_control()
if ctrl2.bell_pitch == 800:
    print("BELL_PITCH_OK")
else:
    print(f"BELL_PITCH_FAIL: got {ctrl2.bell_pitch}")
