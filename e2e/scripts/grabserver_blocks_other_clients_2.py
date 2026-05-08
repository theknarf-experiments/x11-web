import Xlib.display, Xlib.X
d = Xlib.display.Display()
d.grab_server()
d.sync()
print("grab_server=ok")

d.ungrab_server()
d.sync()
print("ungrab_server=ok")

d.close()
