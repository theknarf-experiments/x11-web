from Xlib import display, X, Xatom
d = display.Display()
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 1, 1, 0, screen.root_depth)
clip = d.intern_atom('CLIPBOARD')
# Set selection owner
w.set_selection_owner(clip, X.CurrentTime)
d.sync()
# Get selection owner
owner = d.get_selection_owner(clip)
print(f"owner_matches={owner.id == w.id}")
w.destroy()
d.close()
