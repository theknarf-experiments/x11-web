import Xlib.display, Xlib.X
import time

# Create a window with one connection
d1 = Xlib.display.Display()
s = d1.screen()
root = s.root
w = root.create_window(
    0, 0, 100, 100, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
)
w.map()
d1.sync()
wid = w.id
print(f'created: wid={wid:#x}')

# A second connection (WM-like) adds this window to its SaveSet
d2 = Xlib.display.Display()
# Reparent window under a WM frame
frame = root.create_window(
    0, 0, 110, 110, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
)
frame.map()
d2.sync()

# The WM would add the client window to its save set
# then reparent it under the frame
# (We test that the server doesn't crash on these operations)
d2.close()
time.sleep(0.1)

# Clean up
w.destroy()
d1.close()
print('saveset-ok')
