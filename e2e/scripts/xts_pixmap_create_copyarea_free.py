from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Create a pixmap
pix = root.create_pixmap(100, 100, screen.root_depth)
gc = root.create_gc(foreground=0xFF0000)
# Draw to pixmap
pix.fill_rectangle(gc, 0, 0, 100, 100)
d.sync()
# Create window and copy pixmap to it
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent, background_pixel=0)
w.map()
d.sync()
w.copy_area(gc, pix, 0, 0, 100, 100, 0, 0)
d.sync()
# Clean up
gc.free()
pix.free()
w.destroy()
d.sync()
print("PASS: pixmap create/draw/copy/free cycle completed")
d.close()
