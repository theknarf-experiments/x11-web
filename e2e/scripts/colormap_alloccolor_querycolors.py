import Xlib.display
d = Xlib.display.Display(":99")
screen = d.screen()
cmap = screen.default_colormap
# AllocColor with exact RGB values
r = cmap.alloc_color(0xFFFF, 0x0000, 0x0000)
print(f"RED_PIXEL={r.pixel:#x}")
assert r.pixel != 0, "Alloc red failed"
# AllocNamedColor
r2 = cmap.alloc_named_color("blue")
print(f"BLUE_PIXEL={r2.pixel:#x}")
assert r2.pixel != 0, "Alloc blue failed"
# QueryColors
colors = cmap.query_colors([r.pixel, r2.pixel])
assert len(colors) == 2, f"Expected 2 colors, got {len(colors)}"
print(f"COLOR0=({colors[0].red:#x},{colors[0].green:#x},{colors[0].blue:#x})")
print("COLORMAP_OK")
d.close()
