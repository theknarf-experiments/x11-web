import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()

# CreateWindow is a void request, so the error comes back asynchronously
# and python-xlib does NOT raise it from sync() — it dispatches via the
# global error handler instead. Install a handler that captures the
# error code so the test can observe it.
captured = {}
def on_error(err, request):
    captured["code"] = getattr(err, "code", None)
    captured["type"] = type(err).__name__
d.set_error_handler(on_error)

w = screen.root.create_window(0, 0, 0, 0, 0, screen.root_depth)
d.sync()
# python-xlib's RANDR module overlays BadRRModeError on top of code=2
# (BadValue) once RANDR is registered. Normalise either spelling.
code = captured.get("code")
if code == 2:
    print("result=BAD_VALUE")
elif code is None:
    print("result=NO_ERROR")
else:
    print(f"result=OTHER:{captured.get('type')}:code={code}")

d.close()
