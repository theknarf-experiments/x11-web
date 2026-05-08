import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask)
w.map()
d.sync()

# Grab pointer
status = w.grab_pointer(False, Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync, Xlib.X.NONE, Xlib.X.NONE,
    Xlib.X.CurrentTime)
print(f"grab_status={status}")
if status == Xlib.X.GrabSuccess:
    print("GRAB_OK")

# Ungrab
d.ungrab_pointer(Xlib.X.CurrentTime)
d.sync()
print("UNGRAB_OK")

d.close()
