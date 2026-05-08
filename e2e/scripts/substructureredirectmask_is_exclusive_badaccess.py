import Xlib.display, Xlib.X, Xlib.error
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()
screen = d1.screen()

# python-xlib's default error handler prints to stderr without raising,
# so async errors from no-reply requests (like ChangeWindowAttributes)
# never surface as exceptions in the test. Capture them on d2 instead.
d2_errors = []
d2.set_error_handler(lambda err, _req: d2_errors.append(err))

# First client grabs SubstructureRedirect on root.
screen.root.change_attributes(event_mask=Xlib.X.SubstructureRedirectMask)
d1.sync()
print("first_grab=ok")

# Second client should get BadAccess.
d2.screen().root.change_attributes(event_mask=Xlib.X.SubstructureRedirectMask)
d2.sync()
if any(isinstance(e, Xlib.error.BadAccess) for e in d2_errors):
    print("second_grab=BadAccess")
elif d2_errors:
    print(f"second_grab=error_{type(d2_errors[0]).__name__}")
else:
    print("second_grab=should_have_failed")

d1.close()
d2.close()
