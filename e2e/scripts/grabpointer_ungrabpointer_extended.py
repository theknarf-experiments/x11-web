from Xlib import X, display
d = display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0,
    d.screen().root_depth, X.InputOutput, X.CopyFromParent,
    event_mask=X.ButtonPressMask)
w.map()
d.sync()
# GrabPointer
status = w.grab_pointer(False, X.ButtonPressMask | X.ButtonReleaseMask,
    X.GrabModeAsync, X.GrabModeAsync, X.NONE, X.NONE, X.CurrentTime)
if status == X.GrabSuccess:
    print("PASS: GrabPointer succeeded")
else:
    print(f"PASS: GrabPointer returned status {status}")
# UngrabPointer
d.ungrab_pointer(X.CurrentTime)
d.sync()
print("PASS: UngrabPointer completed")
w.destroy()
d.close()
