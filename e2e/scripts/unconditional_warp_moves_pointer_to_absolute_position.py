import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Warp to absolute position (100, 200) relative to root
root.warp_pointer(100, 200)
d.sync()

# Query pointer position
qp = root.query_pointer()
dx = abs(qp.root_x - 100)
dy = abs(qp.root_y - 200)
if dx <= 1 and dy <= 1:
    print("ABSOLUTE_WARP_OK")
else:
    print(f"ABSOLUTE_WARP_FAIL: expected (100,200), got ({qp.root_x},{qp.root_y})")
