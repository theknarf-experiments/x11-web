import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask)
w2 = screen.root.create_window(200, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask)
w1.map()
w2.map()
d.sync()

# Set focus to w1
d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus = d.get_input_focus()
if focus.focus.id == w1.id:
    print("FOCUS_W1_OK")

# Switch focus to w2
d.set_input_focus(w2, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus = d.get_input_focus()
if focus.focus.id == w2.id:
    print("FOCUS_W2_OK")

d.close()
