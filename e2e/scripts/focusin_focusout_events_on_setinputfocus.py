import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w2 = screen.root.create_window(200, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w1.map()
w2.map()
d.sync()

# Focus w1
d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus1 = d.get_input_focus()
print(f"focus_w1={focus1.focus.id == w1.id}")

# Now focus w2
d.set_input_focus(w2, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus2 = d.get_input_focus()
print(f"focus_w2={focus2.focus.id == w2.id}")

import time
time.sleep(0.1)

# Drain events - should have FocusIn/FocusOut
focus_events = []
while d.pending_events() > 0:
    ev = d.next_event()
    if ev.type in (9, 10):  # FocusIn=9, FocusOut=10
        focus_events.append(ev.type)

print(f"got_focus_in={9 in focus_events}")
print(f"got_focus_out={10 in focus_events}")

w1.destroy()
w2.destroy()
d.close()
