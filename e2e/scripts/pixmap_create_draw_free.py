from Xlib import X, display
d = display.Display()
screen = d.screen()
root = screen.root
# Create a pixmap
pm = root.create_pixmap(100, 100, screen.root_depth)
print(f"PASS: CreatePixmap id=0x{pm.id:x}")
# Create GC and draw on pixmap
gc = pm.create_gc(foreground=screen.white_pixel)
pm.fill_rectangle(gc, 0, 0, 100, 100)
d.sync()
print("PASS: drew on pixmap")
# CopyArea from pixmap to window
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent, background_pixel=screen.black_pixel)
w.map()
d.sync()
w.copy_area(gc, pm, 0, 0, 100, 100, 0, 0)
d.sync()
print("PASS: CopyArea pixmap to window")
# Free
gc.free()
pm.free()
w.destroy()
d.sync()
print("PASS: all resources freed")
d.close()
