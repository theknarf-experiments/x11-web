import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()

# Create a window to own the clipboard
w = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth,
    event_mask=Xlib.X.PropertyChangeMask)
w.map()
d.sync()

# Set selection owner
clipboard = d.intern_atom('CLIPBOARD')
w.set_selection_owner(clipboard, Xlib.X.CurrentTime)
d.sync()

# Verify ownership
owner = d.get_selection_owner(clipboard)
print(f"owns_clipboard={owner == w.id or (hasattr(owner, 'id') and owner.id == w.id)}")

# Set a property with data
test_data = b"Hello from X11 clipboard test! " * 100  # ~3KB
test_atom = d.intern_atom('_CLIP_TEST')
w.change_property(test_atom, Xlib.Xatom.STRING, 8, test_data)
d.sync()

# Read it back
prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
if prop:
    print(f"data_len={len(prop.value)}")
    print(f"data_matches={prop.value == test_data}")
else:
    print("data_matches=False")

w.destroy()
d.close()
