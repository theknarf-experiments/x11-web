import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Create an override-redirect window
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    override_redirect=True)

# Map it — should succeed directly even if WM has redirect
w.map()
d.sync()

# Verify it's mapped
attrs = w.get_attributes()
if attrs.map_state == 2:  # IsViewable
    print("OR_MAP_OK")
else:
    print(f"OR_MAP_STATE={attrs.map_state}")

d.close()
