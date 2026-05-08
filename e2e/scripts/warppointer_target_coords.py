"""
`WarpPointer` with absolute coordinates should land the pointer
within ±1 pixel of the target on the next `QueryPointer`.
"""

import Xlib.display

d = Xlib.display.Display()
root = d.screen().root

d.warp_pointer(200, 150)
d.sync()

result = root.query_pointer()
x, y = result.root_x, result.root_y
if abs(x - 200) <= 1 and abs(y - 150) <= 1:
    print("WARP_OK")
else:
    print(f"WARP_FAIL: got ({x},{y}) expected (200,150)")
