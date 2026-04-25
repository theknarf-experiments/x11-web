import Xlib.display, Xlib.X, Xlib.Xatom, time
d = Xlib.display.Display(":99")
root = d.screen().root
# Owner window
owner = root.create_window(0, 0, 1, 1, 0, 0,
    event_mask=Xlib.X.PropertyChangeMask)
d.sync()
# Claim CLIPBOARD ownership
clip_atom = d.intern_atom("CLIPBOARD")
owner.set_selection_owner(clip_atom, Xlib.X.CurrentTime)
d.sync()
sel_owner = d.get_selection_owner(clip_atom)
assert sel_owner == owner, f"Owner mismatch: {sel_owner} vs {owner}"
print("SELECTION_OWNER_OK")
owner.destroy()
d.close()
