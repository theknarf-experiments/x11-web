import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create window with save_under=True
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    save_under=1)
d.sync()

attrs = w.get_attributes()
print(f"save_under={bool(attrs.save_under)}")

w.destroy()
d.close()
