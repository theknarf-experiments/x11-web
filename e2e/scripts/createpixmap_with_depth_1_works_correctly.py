from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Create depth-1 pixmap (bitmap)
pix = root.create_pixmap(32, 32, 1)
print(f"pixmap_created={pix.id > 0}")
# Create GC for depth-1 drawable
gc = pix.create_gc(foreground=1, background=0)
# Draw a point
pix.fill_rectangle(gc, 0, 0, 32, 32)
print("fill_ok=True")
gc.free()
pix.free()
d.close()
