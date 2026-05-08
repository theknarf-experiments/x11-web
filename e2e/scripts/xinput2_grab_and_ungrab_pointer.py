import Xlib.display
import Xlib.X
d = Xlib.display.Display()
screen = d.screen()
root = screen.root

# Try grabbing the pointer
status = root.grab_pointer(
    True,
    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask | Xlib.X.PointerMotionMask,
    Xlib.X.GrabModeAsync,
    Xlib.X.GrabModeAsync,
    0,  # confine_to
    0,  # cursor
    Xlib.X.CurrentTime
)
print(f"grab_status={status}")

# Ungrab
d.ungrab_pointer(Xlib.X.CurrentTime)
d.sync()
print("ungrab=ok")
d.close()
