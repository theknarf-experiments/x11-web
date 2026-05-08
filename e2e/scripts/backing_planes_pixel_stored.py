"""
Verifies that the server stores `backing_planes` and
`backing_pixel` from `CreateWindow` and reports them back via
`GetWindowAttributes`. Note that python-xlib renames the
`backing_planes` protocol field to `backing_bit_planes` on the
reply object.
"""

from Xlib import X, display

d = display.Display()
root = d.screen().root
w = root.create_window(
    0, 0, 100, 100, 0, d.screen().root_depth,
    X.InputOutput, X.CopyFromParent,
    backing_store=X.Always,
    backing_planes=0xFF0000,
    backing_pixel=0x00FF00,
)
attrs = w.get_attributes()
print(f"planes={attrs.backing_bit_planes:#x}")
print(f"pixel={attrs.backing_pixel:#x}")
w.destroy()
d.close()
