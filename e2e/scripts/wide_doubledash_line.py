"""
LineDoubleDash should draw the dash gaps in the GC's `background`
colour rather than leaving them as the underlying surface. Reads
back pixels from both an on-dash and an off-dash position to
confirm both are coloured.
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

# DoubleDash: fg=white, bg=red.
gc = win.create_gc(
    foreground=0xFFFFFF,
    background=0xFF0000,
    line_width=5,
    line_style=Xlib.X.LineDoubleDash,
    dash_list=[10, 10],
    dash_offset=0,
)

win.line(gc, 10, 25, 190, 25)
d.sync()
time.sleep(0.1)

img = win.get_image(10, 25, 180, 1, Xlib.X.ZPixmap, 0xFFFFFF)
data = img.data

px_on = int.from_bytes(data[5 * 4:5 * 4 + 3], "little")
px_off = int.from_bytes(data[15 * 4:15 * 4 + 3], "little")

print(f"on_pixel={px_on:#x} off_pixel={px_off:#x}")
if px_on > 0 and px_off > 0:
    print("PASS: DoubleDash draws both foreground and background")
else:
    print("FAIL: DoubleDash missing pixels")
d.close()
