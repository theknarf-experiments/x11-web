import Xlib.display, Xlib.X
import Xlib.protocol.request as req
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

# Create a window at (100, 100) size 200x200
w = root.create_window(100, 100, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# Conditional WarpPointer with src_window+src rect: only warps if pointer
# is currently inside that rect. python-xlib's Window.warp_pointer wrapper
# always uses src_window=None; the underlying request supports it though.
def conditional_warp(src_id, dx, dy):
    req.WarpPointer(display=d.display,
        src_window=src_id, dst_window=0,
        src_x=0, src_y=0, src_width=200, src_height=200,
        dst_x=dx, dst_y=dy)

# First test: pointer inside src window -> warp should happen
root.warp_pointer(200, 200)  # inside the window
d.sync()
conditional_warp(w.id, 10, 10)
d.sync()

qp = root.query_pointer()
# Pointer was at (200,200), should now be at (210,210) since it was inside the window
dx = abs(qp.root_x - 210)
dy = abs(qp.root_y - 210)
if dx <= 2 and dy <= 2:
    print("CONDITIONAL_WARP_INSIDE_OK")
else:
    print(f"CONDITIONAL_WARP_INSIDE_FAIL: expected ~(210,210), got ({qp.root_x},{qp.root_y})")

# Second test: pointer outside src window -> warp should NOT happen
root.warp_pointer(50, 50)  # outside the window (100,100,200,200)
d.sync()
conditional_warp(w.id, 99, 99)
d.sync()

qp2 = root.query_pointer()
# Pointer should still be at (50,50) since it was outside the window
dx2 = abs(qp2.root_x - 50)
dy2 = abs(qp2.root_y - 50)
if dx2 <= 2 and dy2 <= 2:
    print("CONDITIONAL_WARP_OUTSIDE_OK")
else:
    print(f"CONDITIONAL_WARP_OUTSIDE_FAIL: expected ~(50,50), got ({qp2.root_x},{qp2.root_y})")
