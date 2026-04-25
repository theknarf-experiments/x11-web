from Xlib import X, display
d = display.Display(":99")
root = d.screen().root
w = root.create_window(0, 0, 200, 200, 0, d.screen().root_depth,
    event_mask=X.ExposureMask)
w.map()
d.sync()
# Create GC with graphics_exposures=True
gc = w.create_gc(graphics_exposures=True)
# CopyArea within bounds - should get NoExposure
w.copy_area(gc, w, 0, 0, 50, 50, 10, 10)
d.sync()
import time; time.sleep(0.1)
# Check pending events
while d.pending_events():
    ev = d.next_event()
    if ev.type == X.NoExpose:
        print("no-exposure-received")
    elif ev.type == X.GraphicsExpose:
        print("graphics-exposure-received")
w.destroy()
d.close()
print("copy-area-test-done")
