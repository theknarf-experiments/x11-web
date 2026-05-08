import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()

try:
    w = screen.root.create_window(0, 0, 0, 0, 0, screen.root_depth)
    d.sync()
    print("result=NO_ERROR")
except Xlib.error.BadValue:
    print("result=BAD_VALUE")
except Xlib.error.XError as e:
    # python-xlib's RANDR module (incorrectly) overlays BadRRModeError
    # on top of code=2 (BadValue) once RANDR is registered. The server-
    # side error is still a plain BadValue — just normalise.
    if getattr(e, "code", None) == 2:
        print("result=BAD_VALUE")
    else:
        print(f"result=OTHER:{type(e).__name__}:code={getattr(e, 'code', None)}")
except Exception as e:
    print(f"result=OTHER:{type(e).__name__}")

d.close()
