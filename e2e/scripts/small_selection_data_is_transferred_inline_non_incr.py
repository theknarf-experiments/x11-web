import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
root = d.screen().root

# Create a window that owns PRIMARY
w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set small clipboard data via property
small_data = b"Hello, clipboard!"
w.change_property(Xlib.Xatom.STRING, Xlib.Xatom.STRING, 8, small_data)
d.sync()

# Read it back
prop = w.get_full_property(Xlib.Xatom.STRING, Xlib.X.AnyPropertyType)
if prop and prop.value == small_data:
    print("SMALL_TRANSFER_OK")
else:
    print(f"SMALL_TRANSFER_FAIL: got {prop}")
