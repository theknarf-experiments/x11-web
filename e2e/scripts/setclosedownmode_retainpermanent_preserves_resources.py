import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Set close-down mode to RetainPermanent
d.set_close_down_mode(Xlib.X.RetainPermanent)
d.sync()
print("close_down_mode_set=True")

d.close()
