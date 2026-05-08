import Xlib.display, Xlib.X, time
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(10, 10, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()
time.sleep(0.3)

# Configure window to new size and position
w.configure(x=100, y=100, width=300, height=200)
d.sync()
time.sleep(0.3)

geom = w.get_geometry()
print(f"new_width={geom.width}")
print(f"new_height={geom.height}")
print(f"configure_ok={geom.width == 300 and geom.height == 200}")

w.destroy()
d.close()
