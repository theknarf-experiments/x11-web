from Xlib import display, X
d = display.Display()
root = d.screen().root
# Query initial modifier state
state = root.query_pointer()
# Modifiers should be 0 initially (no keys pressed)
print(f"initial_mods={state.mask & 0xFF}")
d.close()
