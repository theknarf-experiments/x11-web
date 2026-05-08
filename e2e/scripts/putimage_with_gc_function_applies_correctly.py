import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
root = screen.root
# Create a pixmap, draw with specific GC function
pm = root.create_pixmap(10, 10, screen.root_depth)
gc_xor = root.create_gc(function=Xlib.X.GXxor, foreground=0xFFFFFF)
# Fill initial pixels
gc_copy = root.create_gc(function=Xlib.X.GXcopy, foreground=0xFF0000)
pm.fill_rectangle(gc_copy, 0, 0, 10, 10)
# XOR should invert
pm.fill_rectangle(gc_xor, 0, 0, 10, 10)
d.sync()
print("gc_function_applied=True")
pm.free()
gc_xor.free()
gc_copy.free()
d.close()
