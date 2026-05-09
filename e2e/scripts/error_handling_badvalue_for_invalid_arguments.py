"""Server returns BadValue (code=2) for an out-of-range CreatePixmap depth.

CreatePixmap is a void request, so python-xlib delivers the error
asynchronously via the global error handler. RANDR's loaded BadRRMode
type also has code=2, so we match by the underlying numeric code rather
than the class name.

Using `bit_gravity=255` (the previous test) doesn't work because
python-xlib validates that value client-side and raises a Python
`ValueError` before any bytes hit the wire.
"""

import Xlib.display
import Xlib.X
import Xlib.error

d = Xlib.display.Display()
screen = d.screen()

captured = {}


def on_error(err, request):
    captured["code"] = getattr(err, "code", None)


d.set_error_handler(on_error)

# Depth 200 is well outside the legal range — server should reply
# BadValue.
screen.root.create_pixmap(10, 10, 200)
d.sync()

if captured.get("code") == 2:
    print("error=BadValue")
elif captured.get("code") is None:
    print("error=none")
else:
    print(f"error=other:code={captured.get('code')}")

d.close()
