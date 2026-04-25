import Xlib.display, Xlib.X
d = Xlib.display.Display(":99")
screen = d.screen()
root = screen.root
COUNT = 100
for i in range(COUNT):
    w = root.create_window(i % 50, i % 50, 50, 50, 0, screen.root_depth)
    w.map()
    d.sync()
    w.destroy()
    d.sync()
print(f"CREATED_AND_DESTROYED={COUNT}")
print("RAPID_WINDOW_OK")
d.close()
