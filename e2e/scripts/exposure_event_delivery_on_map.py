import Xlib.display, Xlib.X
import time

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(
    0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask,
    background_pixel=screen.white_pixel,
)
w.map()
d.sync()

# Wait a bit for exposure events to arrive
time.sleep(0.5)

expose_count = 0
while True:
    ev = d.pending_events()
    if ev == 0:
        break
    e = d.next_event()
    if e.type == Xlib.X.Expose:
        expose_count += 1

print(f"expose_count={expose_count}")
print(f"got_expose={expose_count > 0}")

w.destroy()
d.close()
