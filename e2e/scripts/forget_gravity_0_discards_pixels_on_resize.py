import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with ForgetGravity (0) - default
w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
    bit_gravity=0)
w.map()
d.sync()

# Draw something
gc = w.create_gc(foreground=0xFF0000)
w.fill_rectangle(gc, 0, 0, 50, 50)
d.sync()

# Resize - with ForgetGravity, Expose should be generated
w.configure(width=100, height=100)
d.sync()

import time
time.sleep(0.1)

print("forget_gravity_resize=ok")
w.destroy()
d.close()
