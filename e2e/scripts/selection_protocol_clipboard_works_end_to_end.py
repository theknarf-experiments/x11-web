import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth)
d.sync()

clipboard = d.intern_atom('CLIPBOARD')
targets = d.intern_atom('TARGETS')

# Set selection owner
w.set_selection_owner(clipboard, Xlib.X.CurrentTime)
d.sync()

# Verify we own it
owner = d.get_selection_owner(clipboard)
print(f"selection_owner={owner == w}")

w.destroy()
d.close()
