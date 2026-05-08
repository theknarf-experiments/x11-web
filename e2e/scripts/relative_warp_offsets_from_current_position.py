import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# First warp to a known position
root.warp_pointer(200, 200)
d.sync()

# Now relative warp: move by (+50, +30)
d.warp_pointer(50, 30)
d.sync()

qp = root.query_pointer()
dx = abs(qp.root_x - 250)
dy = abs(qp.root_y - 230)
if dx <= 1 and dy <= 1:
    print("RELATIVE_WARP_OK")
else:
    print(f"RELATIVE_WARP_FAIL: expected (250,230), got ({qp.root_x},{qp.root_y})")
