import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

src = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
dst = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
src.map()
dst.map()
d.sync()

gc = src.create_gc(foreground=0xFF0000)
src.fill_rectangle(gc, 0, 0, 50, 50)
d.sync()

# CopyArea from src to dst
dst.copy_area(gc, src, 0, 0, 50, 50, 10, 10)
d.sync()
print("copy_area=ok")

src.destroy()
dst.destroy()
d.close()
