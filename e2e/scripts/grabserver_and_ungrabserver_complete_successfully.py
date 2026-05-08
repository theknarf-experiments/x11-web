import Xlib.display
d = Xlib.display.Display()

d.grab_server()
d.sync()
print("server_grabbed=True")

d.ungrab_server()
d.sync()
print("server_ungrabbed=True")

d.close()
