import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
root = d.screen().root

# Create a window and set CLIPBOARD ownership
w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.PropertyChangeMask)
w.map()
d.sync()

# Intern atoms
clipboard = d.intern_atom("CLIPBOARD")
targets_atom = d.intern_atom("TARGETS")
timestamp_atom = d.intern_atom("TIMESTAMP")
utf8 = d.intern_atom("UTF8_STRING")

# Set selection owner (set_selection_owner is on the Window, not Display)
w.set_selection_owner(clipboard, Xlib.X.CurrentTime)
d.sync()

owner = d.get_selection_owner(clipboard)
if owner == w:
    print("SELECTION_OWNER_OK")
else:
    print(f"SELECTION_OWNER_FAIL: expected {w}, got {owner}")
