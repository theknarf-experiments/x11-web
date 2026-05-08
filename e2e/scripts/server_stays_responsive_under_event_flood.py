from Xlib import X, display, Xatom

d = display.Display()
screen = d.screen()
root = screen.root

# Create a window and flood it with property changes (generates PropertyNotify events)
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent,
    event_mask=X.PropertyChangeMask)
w.map()
d.sync()

atom = d.intern_atom("_TEST_FLOOD")
for i in range(1000):
    w.change_property(atom, Xatom.STRING, 8, f"value{i}".encode())

d.sync()

# Verify server is still responding
info = d.get_display_name()
print(f"flood_ok=True display={info}")
w.destroy()
d.close()
