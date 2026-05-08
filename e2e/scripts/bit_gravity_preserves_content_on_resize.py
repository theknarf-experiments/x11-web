import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with SouthEast bit_gravity (9)
w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    bit_gravity=9,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# Verify via GetWindowAttributes
attrs = w.get_attributes()
print(f"bit_gravity={attrs.bit_gravity}")

w.destroy()
d.close()
