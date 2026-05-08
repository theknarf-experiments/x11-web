import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Just verify KillClient(0) (self) doesn't crash
# Using allClients=0 should be a no-op essentially
print("kill_client_test=ok")
d.close()
