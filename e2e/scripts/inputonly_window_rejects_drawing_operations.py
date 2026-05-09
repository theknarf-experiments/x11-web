import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()

# Create InputOnly window (class=2, depth=0)
w = screen.root.create_window(0, 0, 100, 100, 0, 0,
    Xlib.X.InputOnly)
w.map()
d.sync()

# CreateGC is a void request — python-xlib does NOT raise BadMatch synchronously
# from sync(). Install a global error handler that records the error class so the
# test can observe it (same pattern as zero_size_window_creation_is_rejected).
captured = {}
def on_error(err, request):
    captured["type"] = type(err).__name__
d.set_error_handler(on_error)

# Try to create a GC on InputOnly — should fail with BadMatch
gc = w.create_gc()
d.sync()

if captured.get("type") == "BadMatch":
    print("gc_create=BadMatch")
elif "type" in captured:
    print(f"gc_create=other:{captured['type']}")
else:
    print("gc_create=should_have_failed")

w.destroy()
d.close()
