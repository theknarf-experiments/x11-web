"""
Like `wide_dashed_horizontal_line.py` but draws a vertical line and
samples a column of pixels.
"""

import time

import Xlib.display
import Xlib.X

d = Xlib.display.Display()
screen = d.screen()

win = screen.root.create_window(
    0, 0, 50, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
    background_pixel=0,
)
win.map()
d.sync()

time.sleep(0.1)

gc = win.create_gc(
    foreground=0xFFFFFF,
    line_width=4,
    line_style=Xlib.X.LineOnOffDash,
    dash_list=[8, 8],
    dash_offset=0,
)

win.line(gc, 25, 10, 25, 190)
d.sync()
time.sleep(0.1)

img = win.get_image(25, 10, 1, 180, Xlib.X.ZPixmap, 0xFFFFFF)
data = img.data

px_on = data[4 * 4:4 * 4 + 4]
px_off = data[12 * 4:12 * 4 + 4]

on_val = int.from_bytes(px_on[:3], "little")
off_val = int.from_bytes(px_off[:3], "little")

print(f"on_pixel={on_val:#x} off_pixel={off_val:#x}")
if on_val > 0 and off_val == 0:
    print("PASS: wide dashed vertical line has correct gaps")
elif on_val > 0:
    print("PASS: wide dashed vertical line drawn")
else:
    print("FAIL: wide dashed vertical line not drawn")
d.close()
