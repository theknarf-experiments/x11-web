import Xlib.display, Xlib.X, Xlib.error
caught_error = None
def error_handler(err, req):
    global caught_error
    caught_error = err

d = Xlib.display.Display()
d.set_error_handler(error_handler)
screen = d.screen()
gc = screen.root.create_gc()

gc.set_dashes(0, [4, 0, 2])  # 0 in dash list is invalid
d.sync()

if caught_error is not None and caught_error.code == 2:
    print("result=BAD_VALUE")
elif caught_error is not None:
    print(f"result=OTHER_ERROR:code={caught_error.code}")
else:
    print("result=NO_ERROR")

gc.free()
d.close()
