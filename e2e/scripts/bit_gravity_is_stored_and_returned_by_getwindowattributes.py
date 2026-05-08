import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with bit_gravity=SouthEast (9)
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    bit_gravity=9)
d.sync()

attrs = w.get_attributes()
print(f"bit_gravity={attrs.bit_gravity}")

# Change to Center (5)
w.change_attributes(bit_gravity=5)
d.sync()
attrs2 = w.get_attributes()
print(f"bit_gravity_changed={attrs2.bit_gravity}")

w.destroy()
d.close()
