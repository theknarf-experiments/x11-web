import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(
    0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ButtonPressMask,
)
w.map()
d.sync()

# Grab pointer
status = w.grab_pointer(
    True,  # owner_events
    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync,
    Xlib.X.GrabModeAsync,
    Xlib.X.NONE,  # confine_to
    Xlib.X.NONE,  # cursor
    Xlib.X.CurrentTime,
)
print(f"grab_status={status}")  # 0 = GrabSuccess

# Ungrab
d.ungrab_pointer(Xlib.X.CurrentTime)
d.sync()
print("ungrab_ok=True")

w.destroy()
d.close()
