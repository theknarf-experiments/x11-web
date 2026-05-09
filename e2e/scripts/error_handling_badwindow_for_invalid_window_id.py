"""GetGeometry on a bogus drawable ID returns BadDrawable/BadWindow.

GetGeometry has a reply, so python-xlib raises the protocol error
synchronously from the request — we don't need the async error handler
trick used elsewhere. Use `create_resource_object` so the wrapper has the
underlying `_BaseDisplay` plumbing (constructing `Window(d, id)`
directly stores the high-level Display, which lacks `send_request`).
"""

import Xlib.display
import Xlib.error

d = Xlib.display.Display()
fake = d.create_resource_object("window", 0xDEADBEEF)

try:
    fake.get_geometry()
    print("error=none")
except (Xlib.error.BadDrawable, Xlib.error.BadWindow):
    # Both are valid — GetGeometry takes a Drawable, so an unknown ID is
    # spec'd to be BadDrawable, but some servers return BadWindow.
    print("error=BadWindow")
except Exception as e:
    print(f"error={type(e).__name__}:{e}")

d.close()
