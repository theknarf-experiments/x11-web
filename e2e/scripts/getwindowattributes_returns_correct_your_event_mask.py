import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(
    0, 0, 50, 50, 0,
    screen.root_depth,
    event_mask=Xlib.X.ExposureMask | Xlib.X.KeyPressMask,
)
attrs = w.get_attributes()
# your_event_mask should include the masks we set
mask = attrs.your_event_mask
print(f"your_event_mask={mask}")
has_exposure = bool(mask & Xlib.X.ExposureMask)
has_keypress = bool(mask & Xlib.X.KeyPressMask)
print(f"has_exposure={has_exposure} has_keypress={has_keypress}")
w.destroy()
d.close()
