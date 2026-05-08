import Xlib.display
import Xlib.X
import struct
d = Xlib.display.Display()
screen = d.screen()

# Source window with known content
src = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth,
    window_class=Xlib.X.InputOutput,
    visual=Xlib.X.CopyFromParent)
src.map()

# Destination window
dst = screen.root.create_window(20, 0, 10, 10, 0, screen.root_depth,
    window_class=Xlib.X.InputOutput,
    visual=Xlib.X.CopyFromParent)
dst.map()
d.sync()

gc = src.create_gc(foreground=0xFF0000)
src.fill_rectangle(gc, 0, 0, 10, 10)
d.sync()

# CopyArea from src to dst
gc2 = dst.create_gc()
dst.copy_area(gc2, src, 0, 0, 10, 10, 0, 0)
d.sync()

# Read back from destination
img = dst.get_image(0, 0, 1, 1, Xlib.X.ZPixmap, 0xFFFFFFFF)
if len(img.data) >= 3:
    b, g, r = img.data[0], img.data[1], img.data[2]
    print(f"copy_r={r}")
    print(f"copy_g={g}")
    print(f"copy_b={b}")

src.destroy()
dst.destroy()
d.sync()
print("copy_area=ok")
d.close()
