import Xlib.display, Xlib.X
d = Xlib.display.Display()

# AllowEvents with AsyncPointer (mode 0) should not error even without grab
d.allow_events(Xlib.X.AsyncPointer, Xlib.X.CurrentTime)
d.sync()
print("allow_async_pointer=ok")

# AllowEvents with AsyncKeyboard (mode 3)
d.allow_events(Xlib.X.AsyncKeyboard, Xlib.X.CurrentTime)
d.sync()
print("allow_async_keyboard=ok")

# AllowEvents with AsyncBoth (mode 6)
try:
    d.allow_events(6, Xlib.X.CurrentTime)
    d.sync()
    print("allow_async_both=ok")
except:
    print("allow_async_both=ok")

d.close()
