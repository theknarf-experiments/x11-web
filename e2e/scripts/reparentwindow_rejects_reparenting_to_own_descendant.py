import Xlib.display, Xlib.X, Xlib.error
caught_error = None
def error_handler(err, req):
    global caught_error
    caught_error = err

d = Xlib.display.Display()
d.set_error_handler(error_handler)
screen = d.screen()
parent = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)
child = parent.create_window(10, 10, 100, 100, 0, screen.root_depth)
grandchild = child.create_window(5, 5, 50, 50, 0, screen.root_depth)
d.sync()

parent.reparent(grandchild, 0, 0)
d.sync()

if caught_error is not None and caught_error.code == 8:
    print("result=BAD_MATCH")
elif caught_error is not None:
    print(f"result=OTHER_ERROR:code={caught_error.code}")
else:
    print("result=NO_ERROR")

grandchild.destroy()
child.destroy()
parent.destroy()
d.close()
