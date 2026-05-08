import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.FocusChangeMask)
w.map()
d.sync()

d.set_input_focus(w, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus = d.get_input_focus()
print(f"focus_window={focus.focus.id == w.id}")
print(f"revert_to={focus.revert_to}")

w.destroy()
d.close()
