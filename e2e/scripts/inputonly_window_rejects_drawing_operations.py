import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()

# Create InputOnly window (class=2, depth=0)
w = screen.root.create_window(0, 0, 100, 100, 0, 0,
    Xlib.X.InputOnly)
w.map()
d.sync()

# Try to create a pixmap on InputOnly — should fail with BadMatch
try:
    gc = w.create_gc()
    d.sync()
    print("gc_create=should_have_failed")
except Xlib.error.BadMatch:
    print("gc_create=BadMatch")
except Exception as e:
    print(f"gc_create=error:{type(e).__name__}")

w.destroy()
d.close()
