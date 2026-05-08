import Xlib.display
import Xlib.X
d = Xlib.display.Display()
screen = d.screen()
root = screen.root

# Grab the keyboard
status = root.grab_keyboard(
    True,
    Xlib.X.GrabModeAsync,
    Xlib.X.GrabModeAsync,
    Xlib.X.CurrentTime
)
print(f"kb_grab_status={status}")

# Ungrab
d.ungrab_keyboard(Xlib.X.CurrentTime)
d.sync()
print("kb_ungrab=ok")
d.close()
