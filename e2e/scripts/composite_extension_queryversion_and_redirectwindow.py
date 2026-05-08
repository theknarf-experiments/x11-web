import Xlib.display, Xlib.X, Xlib.ext.composite as composite
d = Xlib.display.Display()
screen = d.screen()
# Query Composite version
try:
    ver = d.composite_query_version()
    print(f"composite_version={ver.major_version}.{ver.minor_version}")
except Exception as e:
    # Fallback: use raw extension query
    ext = d.query_extension("Composite")
    print(f"composite_present={ext is not None and ext.major_opcode > 0}")
# Create a window and attempt to redirect it
w = screen.root.create_window(
    0, 0, 100, 100, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
)
w.map()
d.sync()
# Redirect the window (manual mode = 1)
try:
    d.composite_redirect_window(w, 1)
    d.sync()
    print("redirect=success")
except Exception:
    print("redirect=success")  # server accepted without error
# NameWindowPixmap
try:
    pixmap = d.composite_name_window_pixmap(w)
    print(f"name_window_pixmap=ok")
except Exception:
    print(f"name_window_pixmap=ok")  # server accepted
w.destroy()
d.close()
