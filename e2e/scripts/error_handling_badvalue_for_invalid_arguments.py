import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()

try:
    # bit_gravity > 10 should be BadValue
    w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth,
        Xlib.X.InputOutput, Xlib.X.CopyFromParent,
        bit_gravity=255)
    d.sync()
    print("error=none")
except Xlib.error.BadValue:
    print("error=BadValue")
except Exception as e:
    print(f"error={type(e).__name__}")
d.close()
