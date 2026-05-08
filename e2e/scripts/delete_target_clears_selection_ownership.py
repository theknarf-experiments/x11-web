import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
root = d.screen().root

# Create owner window
w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Take PRIMARY ownership (set_selection_owner is on the Window).
# Predefined atoms live in Xlib.Xatom, not Xlib.X.
w.set_selection_owner(Xlib.Xatom.PRIMARY, Xlib.X.CurrentTime)
d.sync()

owner = d.get_selection_owner(Xlib.Xatom.PRIMARY)
if owner == w:
    print("OWNER_SET_OK")
else:
    print(f"OWNER_SET_FAIL: {owner}")
