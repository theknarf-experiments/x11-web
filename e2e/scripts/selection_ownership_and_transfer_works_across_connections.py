from Xlib import X, display, Xatom
import time

d1 = display.Display()
d2 = display.Display()
root = d1.screen().root

# Create windows for each "client"
w1 = root.create_window(0, 0, 10, 10, 0, d1.screen().root_depth)
w2 = root.create_window(0, 0, 10, 10, 0, d2.screen().root_depth)

# d1 takes PRIMARY selection ownership
primary = d1.intern_atom("PRIMARY")
w1.set_selection_owner(primary, X.CurrentTime)
d1.sync()

# d2 checks owner. get_selection_owner returns a Window object (or 0 if
# unowned), so compare its .id to w1.id.
owner = d2.get_selection_owner(primary)
owner_id = owner.id if owner != 0 else 0
print(f"owner_matches={owner_id == w1.id}")

# Cleanup
w1.destroy()
w2.destroy()
d1.close()
d2.close()
