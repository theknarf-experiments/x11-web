import Xlib.display
import Xlib.X
import Xlib.protocol.event
import sys
import time

d = Xlib.display.Display(':99')
root = d.screen().root

# Create a window to receive events
w = root.create_window(
    0, 0, 200, 200, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    event_mask=(
        Xlib.X.PointerMotionMask |
        Xlib.X.StructureNotifyMask
    ),
)
w.map()
d.sync()
time.sleep(0.3)

start = time.time()
N = 1000
for i in range(N):
    # Use WarpPointer to generate real MotionNotify events
    # Alternate between two positions to ensure actual movement
    x = 50 + (i % 100)
    y = 50 + (i // 10) % 100
    d.warp_pointer(x, y, w, owindow=w)
    if i % 100 == 0:
        d.sync()

d.sync()
elapsed = time.time() - start
print(f"PASS: sent {N} pointer warps in {elapsed:.2f}s")

# Verify server is still alive
tree = root.query_tree()
print(f"PASS: server responsive after event flood")

w.destroy()
d.sync()
d.close()
print("EVENT_FLOOD_OK")
