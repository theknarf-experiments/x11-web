import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(10, 20, 200, 150, 3, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()

# Resize
w.configure(width=300, height=250, x=50, y=60)
d.sync()

import time
time.sleep(0.3)

found = False
while d.pending_events():
    e = d.next_event()
    if e.type == Xlib.X.ConfigureNotify:
        found = True
        print(f"event_window={e.window.id == w.id}")
        print(f"width={e.width}")
        print(f"height={e.height}")
        print(f"border_width={e.border_width}")
        print(f"override={e.override}")
        break

print(f"got_configure_notify={found}")

w.destroy()
d.close()
