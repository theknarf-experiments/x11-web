import Xlib.display, Xlib.X
d = Xlib.display.Display(":99")
screen = d.screen()
root = screen.root
w1 = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w2 = root.create_window(200, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w1.map()
w2.map()
d.sync()
# Set focus to w1
d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()
# Verify focus
focus = d.get_input_focus()
print(f"FOCUS_WINDOW={focus.focus}")
assert focus.focus == w1, f"Focus should be w1"
# Set focus to w2
d.set_input_focus(w2, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()
focus2 = d.get_input_focus()
assert focus2.focus == w2, f"Focus should be w2"
# Check for FocusIn/FocusOut events
got_focus_in = False
got_focus_out = False
while d.pending_events():
    e = d.next_event()
    if e.type == Xlib.X.FocusIn:
        got_focus_in = True
    if e.type == Xlib.X.FocusOut:
        got_focus_out = True
print(f"FOCUS_IN={got_focus_in} FOCUS_OUT={got_focus_out}")
w1.destroy()
w2.destroy()
d.sync()
print("FOCUS_EVENTS_OK")
d.close()
