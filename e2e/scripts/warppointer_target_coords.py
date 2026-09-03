"""
`WarpPointer` with absolute coordinates should land the pointer
within ±1 pixel of the target on the next `QueryPointer`.

NB: the absolute form is `Window.warp_pointer` (dst_window = that
window), *not* `Display.warp_pointer`. python-xlib's own docstring for
the Display method reads "Move the pointer relative its current
position by the offsets (x, y) ... To move the pointer to absolute
coordinates, use Window.warp_pointer()" — it sends dst_window = None,
which the protocol defines as a relative warp. Using it here made this
script assert an absolute result from a relative request, so it only
passed while the server-wide pointer happened to still be at (0, 0).
See `relative_warp_offsets_from_current_position.py` for the deliberate
relative case.
"""

import Xlib.display

d = Xlib.display.Display()
root = d.screen().root

root.warp_pointer(200, 150)
d.sync()

result = root.query_pointer()
x, y = result.root_x, result.root_y
if abs(x - 200) <= 1 and abs(y - 150) <= 1:
    print("WARP_OK")
else:
    print(f"WARP_FAIL: got ({x},{y}) expected (200,150)")
