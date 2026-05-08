import Xlib.display, Xlib.X, Xlib.Xatom
import time

d = Xlib.display.Display()
screen = d.screen()

# Create owner window
owner = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.PropertyChangeMask)
owner.map()
d.sync()

# Set selection owner
sel_atom = d.intern_atom('PRIMARY')
owner.set_selection_owner(sel_atom, Xlib.X.CurrentTime)
d.sync()

# Verify ownership
sel_owner = d.get_selection_owner(sel_atom)
if sel_owner == owner:
    print("SELECTION_OWNER_OK")
else:
    print(f"FAIL: expected owner {owner.id}, got {sel_owner}")

d.close()
