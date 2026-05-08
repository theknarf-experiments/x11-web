import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with backing_store=Always (2)
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    backing_store=2)
d.sync()

attrs = w.get_attributes()
print(f"backing_store={attrs.backing_store}")

# Change to WhenMapped (1)
w.change_attributes(backing_store=1)
d.sync()
attrs2 = w.get_attributes()
print(f"backing_store_changed={attrs2.backing_store}")

w.destroy()
d.close()
