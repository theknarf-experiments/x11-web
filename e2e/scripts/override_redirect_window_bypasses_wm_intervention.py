import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create override-redirect window
w = screen.root.create_window(100, 100, 200, 200, 0, screen.root_depth,
    override_redirect=True)
w.map()
d.sync()

# Check attributes
attrs = w.get_attributes()
print(f"override_redirect={attrs.override_redirect}")
print(f"map_state={attrs.map_state}")

# Override-redirect windows should be mapped immediately (map_state=2)
print(f"immediately_viewable={attrs.map_state == 2}")

w.destroy()
d.close()
