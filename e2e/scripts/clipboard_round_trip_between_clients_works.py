import Xlib.display, Xlib.X, Xlib.Xatom

d1 = Xlib.display.Display()
screen = d1.screen()

# Create owner window
owner = screen.root.create_window(0, 0, 1, 1, 0, screen.root_depth,
    event_mask=Xlib.X.PropertyChangeMask)
owner.map()
d1.sync()

# Set clipboard content
clipboard = d1.intern_atom('CLIPBOARD')
utf8 = d1.intern_atom('UTF8_STRING')

# Set owner
owner.set_selection_owner(clipboard, Xlib.X.CurrentTime)
d1.sync()

# Verify ownership via the same connection
sel_reply = d1.get_selection_owner(clipboard)
# python3-xlib returns a Window resource from get_selection_owner
sel_id = sel_reply.id if hasattr(sel_reply, 'id') else int(sel_reply)
print(f"sel_id={sel_id} owner_id={owner.id}")
print(f"owner_set={sel_id == owner.id}")

owner.destroy()
d1.close()
