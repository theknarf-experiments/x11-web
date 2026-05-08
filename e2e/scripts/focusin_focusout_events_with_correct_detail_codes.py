import Xlib.display, Xlib.X
import time

d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w2 = screen.root.create_window(100, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w1.map()
w2.map()
d.sync()

# Focus w1
d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()
time.sleep(0.2)

# Drain events
while d.pending_events():
    d.next_event()

# Focus w2 — w1 should get FocusOut, w2 should get FocusIn
d.set_input_focus(w2, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()
time.sleep(0.2)

focus_in = False
focus_out = False
while d.pending_events():
    e = d.next_event()
    if e.type == Xlib.X.FocusIn:
        focus_in = True
    elif e.type == Xlib.X.FocusOut:
        focus_out = True

print(f"focus_in={focus_in}")
print(f"focus_out={focus_out}")

w1.destroy()
w2.destroy()
d.close()
