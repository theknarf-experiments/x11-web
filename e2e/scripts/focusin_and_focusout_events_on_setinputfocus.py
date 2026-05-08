import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(10, 10, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask)
w2 = screen.root.create_window(200, 10, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask)
w1.map()
w2.map()
d.sync()

# Set focus to w1
d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

# Set focus to w2
d.set_input_focus(w2, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus_events = 0
while d.pending_events():
    ev = d.next_event()
    if ev.type in (Xlib.X.FocusIn, Xlib.X.FocusOut):
        focus_events += 1

print(f"focus_events_generated={focus_events > 0}")

w1.destroy()
w2.destroy()
d.close()
