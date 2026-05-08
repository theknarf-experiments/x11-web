import Xlib.display, Xlib.X, Xlib.error
caught_error = None
def error_handler(err, req):
    global caught_error
    caught_error = err

d = Xlib.display.Display()
d.set_error_handler(error_handler)
screen = d.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth)
d.sync()

w.reparent(w, 0, 0)
d.sync()

if caught_error is not None and caught_error.code == 8:
    print("result=BAD_MATCH")
elif caught_error is not None:
    print(f"result=OTHER_ERROR:code={caught_error.code}")
else:
    print("result=NO_ERROR")

w.destroy()
d.close()
