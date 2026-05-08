from Xlib import X, display
d = display.Display()
root = d.screen().root

# Create override-redirect window
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    override_redirect=True)
w.map()
d.sync()

# Check window attributes
attrs = w.get_attributes()
print(f"override_redirect={attrs.override_redirect}")
print(f"map_state={attrs.map_state}")

w.destroy()
d.close()
