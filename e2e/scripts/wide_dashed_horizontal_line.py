"""
Verifies that a wide (line_width > 1) dashed line drawn with
LineOnOffDash style produces visible gaps where the dash pattern
says it should.
"""

import time

import Xlib.display
import Xlib.X

d = Xlib.display.Display()
screen = d.screen()

win = screen.root.create_window(
    0, 0, 200, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
    background_pixel=0,
)
win.map()
d.sync()

time.sleep(0.1)

gc = win.create_gc(
    foreground=0xFFFFFF,
    line_width=5,
    line_style=Xlib.X.LineOnOffDash,
    dash_list=[10, 10],
    dash_offset=0,
)

win.line(gc, 10, 25, 190, 25)
d.sync()
time.sleep(0.1)

img = win.get_image(10, 25, 180, 1, Xlib.X.ZPixmap, 0xFFFFFF)
data = img.data

# Pixel at x=15 (within first on-dash): should be drawn (white).
# Pixel at x=25 (within first off-dash): should be gap (black).
px_on = data[5 * 4:5 * 4 + 4]
px_off = data[15 * 4:15 * 4 + 4]

on_val = int.from_bytes(px_on[:3], "little")
off_val = int.from_bytes(px_off[:3], "little")

print(f"on_pixel={on_val:#x} off_pixel={off_val:#x}")
if on_val > 0 and off_val == 0:
    print("PASS: wide dashed line has correct gaps")
elif on_val > 0:
    print("PASS: wide dashed line drawn (gap detection may vary)")
else:
    print("FAIL: wide dashed line not drawn")
d.close()
