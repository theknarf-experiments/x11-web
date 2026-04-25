import Xlib.display
import Xlib.X
import sys
import time

d = Xlib.display.Display(':99')
root = d.screen().root

start = time.time()
N = 200
for i in range(N):
    w = root.create_window(
        0, 0, 50, 50, 0,
        d.screen().root_depth,
        Xlib.X.InputOutput,
        Xlib.X.CopyFromParent,
        event_mask=Xlib.X.StructureNotifyMask,
    )
    w.map()
    d.sync()
    w.unmap()
    w.destroy()
    d.sync()

elapsed = time.time() - start
print(f"PASS: created and destroyed {N} windows in {elapsed:.2f}s")

# Verify the server is still responsive by querying the root window
tree = root.query_tree()
print(f"PASS: server responsive, root has {len(tree.children)} children")

d.close()
print("WINDOW_LIFECYCLE_OK")
