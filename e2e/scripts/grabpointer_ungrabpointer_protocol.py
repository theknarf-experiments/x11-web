import Xlib.display, Xlib.X
d = Xlib.display.Display(":99")
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, 0,
    event_mask=Xlib.X.ButtonPressMask)
w.map()
d.sync()
# Grab pointer
status = w.grab_pointer(True,
    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
    Xlib.X.GrabModeAsync, Xlib.X.GrabModeAsync,
    Xlib.X.NONE, Xlib.X.NONE, Xlib.X.CurrentTime)
print(f"GRAB_STATUS={status}")
assert status == Xlib.X.GrabSuccess, f"Grab failed: {status}"
# Ungrab
d.ungrab_pointer(Xlib.X.CurrentTime)
d.sync()
w.destroy()
d.sync()
print("GRAB_OK")
d.close()
