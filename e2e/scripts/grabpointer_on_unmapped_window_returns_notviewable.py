import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
# Create but do NOT map the window
w = screen.root.create_window(10, 10, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ButtonPressMask)
d.sync()

status = w.grab_pointer(True, Xlib.X.ButtonPressMask,
    Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync, Xlib.X.NONE, Xlib.X.NONE,
    Xlib.X.CurrentTime)
# Status 3 = NotViewable
print(f"grab_status={status}")

w.destroy()
d.close()
