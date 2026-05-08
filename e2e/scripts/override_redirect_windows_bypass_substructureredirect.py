import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root

# Create an override-redirect window
w = root.create_window(50, 50, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    override_redirect=True,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# Verify window is mapped and has override-redirect set
attrs = w.get_attributes()
if attrs.override_redirect:
    print("OVERRIDE_REDIRECT_OK")
else:
    print("OVERRIDE_REDIRECT_FAIL")
