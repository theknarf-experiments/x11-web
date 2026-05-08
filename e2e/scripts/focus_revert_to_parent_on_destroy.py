from Xlib import X, display
d = display.Display()
root = d.screen().root

parent = root.create_window(0, 0, 200, 200, 0, d.screen().root_depth,
    event_mask=X.FocusChangeMask)
parent.map()
d.sync()

child = parent.create_window(10, 10, 50, 50, 0, d.screen().root_depth,
    event_mask=X.FocusChangeMask)
child.map()
d.sync()

# Set focus to child with revert_to=Parent
d.set_input_focus(child, X.RevertToParent, X.CurrentTime)
d.sync()

focus_before = d.get_input_focus()
print(f"focus_before={focus_before.focus.id}")

# Destroy the focused child — focus should revert to parent
child.destroy()
d.sync()

import time
time.sleep(0.1)
focus_after = d.get_input_focus()
focus_id = focus_after.focus.id if hasattr(focus_after.focus, "id") else focus_after.focus
print(f"focus_after={focus_id}")
print(f"parent_id={parent.id}")

parent.destroy()
d.close()
