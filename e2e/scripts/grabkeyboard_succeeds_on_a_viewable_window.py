import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(10, 10, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.KeyPressMask)
w.map()
d.sync()

status = w.grab_keyboard(True, Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync,
    Xlib.X.CurrentTime)
print(f"keyboard_grab_status={status}")
d.ungrab_keyboard(Xlib.X.CurrentTime)
d.sync()

w.destroy()
d.close()
