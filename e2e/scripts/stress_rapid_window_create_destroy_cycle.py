import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

created = 0
for i in range(100):
    w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth)
    w.map()
    d.sync()
    w.destroy()
    d.sync()
    created += 1

print(f"cycles={created}")
d.close()
