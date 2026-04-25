from Xlib import X, display
import time
# Client 1 grabs the server then disconnects
d1 = display.Display()
d1.grab_server()
d1.sync()
d1.close()  # Should release server grab
time.sleep(0.3)
# Client 2 should be able to connect and operate normally
d2 = display.Display()
root = d2.screen().root
w = root.create_window(0, 0, 50, 50, 0, d2.screen().root_depth,
    X.InputOutput, X.CopyFromParent)
w.map()
d2.sync()
w.destroy()
d2.sync()
print("PASS: server grab released on disconnect")
d2.close()
