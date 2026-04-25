import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root
# Test that we can select all standard event masks without error
all_masks = (
    Xlib.X.KeyPressMask | Xlib.X.KeyReleaseMask |
    Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask |
    Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask |
    Xlib.X.PointerMotionMask | Xlib.X.PointerMotionHintMask |
    Xlib.X.Button1MotionMask | Xlib.X.Button2MotionMask |
    Xlib.X.Button3MotionMask | Xlib.X.Button4MotionMask |
    Xlib.X.Button5MotionMask | Xlib.X.ButtonMotionMask |
    Xlib.X.KeymapStateMask | Xlib.X.ExposureMask |
    Xlib.X.VisibilityChangeMask | Xlib.X.StructureNotifyMask |
    Xlib.X.PropertyChangeMask | Xlib.X.ColormapChangeMask |
    Xlib.X.FocusChangeMask
)
w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    event_mask=all_masks)
d.sync()
w.destroy()
d.sync()
d.close()
print(f"PASS: all event masks accepted (0x{all_masks:08x})")
