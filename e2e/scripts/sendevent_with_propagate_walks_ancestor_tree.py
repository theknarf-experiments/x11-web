import Xlib.display, Xlib.X, Xlib.protocol.event
d = Xlib.display.Display()
screen = d.screen()

# Create parent with KeyPress mask
parent = screen.root.create_window(10, 10, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.KeyPressMask)
parent.map()
d.sync()

# Create child without KeyPress mask
child = parent.create_window(5, 5, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ButtonPressMask)
child.map()
d.sync()

print("propagation_setup=ok")

parent.destroy()
d.close()
