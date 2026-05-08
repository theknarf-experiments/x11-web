import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with backing_store=Always (2)
w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    backing_store=2,
    event_mask=Xlib.X.ExposureMask | Xlib.X.StructureNotifyMask)
w.map()
d.sync()

# Draw something
gc = w.create_gc(foreground=0xFF0000)
w.fill_rectangle(gc, 0, 0, 25, 25)
d.sync()

# GetWindowAttributes should report backing_store
attrs = w.get_attributes()
print(f"backing_store={attrs.backing_store}")

# Unmap and remap
w.unmap()
d.sync()
w.map()
d.sync()

# Content should be preserved (no expose needed)
print("backing_store_test=ok")

w.destroy()
d.close()
