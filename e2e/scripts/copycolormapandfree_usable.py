"""
Issues a raw `CopyColormapAndFree` (opcode 80) and confirms the
returned colormap is valid for further allocations.

python-xlib's `Colormap.copy_colormap_and_free` wrapper has an
upstream typo (references `src_cmap` while the parameter is
`scr_cmap`), so we go through `Xlib.protocol.request` directly.
"""

import Xlib.display
import Xlib.protocol.request as req

d = Xlib.display.Display()
src = d.screen().default_colormap

# Allocate red on the source colormap so we can verify the spec'd
# "free source allocations" behaviour on the round-trip side.
src.alloc_color(0xFFFF, 0, 0)

new_id = d.display.allocate_resource_id()
req.CopyColormapAndFree(display=d.display, mid=new_id, src_cmap=src.id)
d.sync()

new_cmap = d.create_resource_object("colormap", new_id)
green = new_cmap.alloc_color(0, 0xFFFF, 0)
print(f"COPY_CMAP_OK pixel={green.pixel:#x}")
