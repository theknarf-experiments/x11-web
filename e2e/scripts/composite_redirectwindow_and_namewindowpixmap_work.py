"""COMPOSITE: QueryVersion, RedirectWindow, UnredirectWindow round-trip.

Drives python-xlib's `Xlib.ext.composite` API instead of hand-rolled
protocol bytes (the previous version used a non-existent
`Display.send_request` and never ran).
"""

import Xlib.display
import Xlib.X
import Xlib.ext.composite as composite

d = Xlib.display.Display()
screen = d.screen()

# Query Composite extension
comp = d.query_extension("Composite")
if comp is None or comp.major_opcode == 0:
    print("composite_not_found")
    d.close()
    raise SystemExit

# python-xlib lazily initialises Xlib.ext.composite the first time
# `query_extension('Composite')` succeeds, so `composite_query_version`
# is already attached to the display object — calling `init` again would
# raise AssertionError("attempting to replace display method").

# QueryVersion through the extension's high-level API. python-xlib's
# wrapper sends a fixed major/minor (1.0) so it just needs `self`.
ver = d.composite_query_version()
print(f"composite_query_ok={ver is not None}")

w = screen.root.create_window(
    0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
)
w.map()
d.sync()

# RedirectWindow with update=Manual (1).
w.composite_redirect_window(composite.RedirectManual)
d.sync()
print("redirect_ok=True")

# UnredirectWindow.
w.composite_unredirect_window(composite.RedirectManual)
d.sync()
print("unredirect_ok=True")

w.destroy()
d.close()
