from Xlib import X, display
d = display.Display(":99")
root = d.screen().root
# Create a test window
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)
w.map()
d.sync()
# Set focus with revert_to=Parent (2)
d.set_input_focus(w, X.RevertToParent, X.CurrentTime)
d.sync()
f = d.get_input_focus()
assert f.revert_to == X.RevertToParent, f"Expected RevertToParent(2), got {f.revert_to}"
# Set focus with revert_to=None (0)
d.set_input_focus(w, X.RevertToNone, X.CurrentTime)
d.sync()
f = d.get_input_focus()
assert f.revert_to == X.RevertToNone, f"Expected RevertToNone(0), got {f.revert_to}"
# Set focus with revert_to=PointerRoot (1)
d.set_input_focus(w, X.RevertToPointerRoot, X.CurrentTime)
d.sync()
f = d.get_input_focus()
assert f.revert_to == X.RevertToPointerRoot, f"Expected RevertToPointerRoot(1), got {f.revert_to}"
w.destroy()
d.close()
print("focus-revert-test-pass")
