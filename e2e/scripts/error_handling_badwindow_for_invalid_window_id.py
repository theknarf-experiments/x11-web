import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()

try:
    # Try to get geometry of a non-existent window
    from Xlib.xobject.drawable import Window
    bad_wid = 0xDEADBEEF
    fake = Window(d, bad_wid)
    geo = fake.get_geometry()
    print("error=none")
except Xlib.error.BadWindow:
    print("error=BadWindow")
except Exception as e:
    print(f"error={type(e).__name__}")
d.close()
