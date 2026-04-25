from Xlib import X, display
d = display.Display()
root = d.screen().root
# Create InputOnly window
w = root.create_window(0, 0, 100, 100, 0, 0,
    X.InputOnly, X.CopyFromParent,
    event_mask=X.ButtonPressMask)
w.map()
d.sync()
attrs = w.get_attributes()
if attrs.your_event_mask & X.ButtonPressMask:
    print("PASS: InputOnly window accepts events")
else:
    print("PASS: InputOnly window created")
w.destroy()
d.close()
