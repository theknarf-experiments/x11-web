from Xlib import display, X, Xatom
d = display.Display()
screen = d.screen()
root = screen.root
# Create a window to own the selection
w = root.create_window(0, 0, 1, 1, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent)
w.map()
d.sync()
# Set selection owner
clipboard = d.intern_atom("CLIPBOARD")
w.set_selection_owner(clipboard, X.CurrentTime)
d.sync()
# Verify we own it. python-xlib returns a Window object from
# get_selection_owner, not a raw XID — compare via .id.
owner = d.get_selection_owner(clipboard)
owner_id = owner.id if hasattr(owner, "id") else owner
if owner_id == w.id:
    print("PASS: SetSelectionOwner + GetSelectionOwner round-trip")
else:
    print(f"FAIL: expected owner={w.id:#x}, got {owner_id:#x}")
w.destroy()
d.close()
