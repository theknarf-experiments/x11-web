import Xlib.display, Xlib.X
d = Xlib.display.Display()
s = d.screen()
root = s.root
# Create window with backing store
w = root.create_window(
    10, 10, 100, 100, 0,
    s.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    backing_store=Xlib.X.Always,
    event_mask=Xlib.X.ExposureMask | Xlib.X.StructureNotifyMask,
)
w.map()
d.sync()
# Draw something
gc = w.create_gc(foreground=0xFF0000)
w.fill_rectangle(gc, 0, 0, 50, 50)
d.sync()
# Unmap and remap
w.unmap()
d.sync()
import time; time.sleep(0.1)
w.map()
d.sync()
import time; time.sleep(0.1)
# Verify window is still mapped
attrs = w.get_attributes()
print(f'backing-store: map_state={attrs.map_state}')
w.destroy()
gc.free()
d.close()
