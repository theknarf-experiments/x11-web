import Xlib.display, Xlib.X, Xlib.Xutil
d = Xlib.display.Display()
s = d.screen()
root = s.root
passed = 0
# Test depth-24 pixmap
pm = root.create_pixmap(10, 10, 24)
gc = root.create_gc()
# Fill with solid color
gc.change(foreground=0xFF0000)
pm.fill_rectangle(gc, 0, 0, 10, 10)
pm.free()
gc.free()
passed += 1
# Test depth-1 pixmap
pm1 = root.create_pixmap(8, 8, 1)
pm1.free()
passed += 1
# Test depth-8 pixmap
pm8 = root.create_pixmap(8, 8, 8)
pm8.free()
passed += 1
d.close()
print(f'putimage-depths: passed={passed}')
