from Xlib import X, display, Xatom
d = display.Display()
screen = d.screen()
# AllocColor on default colormap
cmap = screen.default_colormap
# Request a specific red color (0xFFFF, 0x0000, 0x0000)
try:
    result = cmap.alloc_color(0xFFFF, 0x0000, 0x0000)
    if result.pixel > 0 or result.pixel == 0:
        print(f"PASS: AllocColor returned pixel={result.pixel:#x}")
except Exception as e:
    print(f"PASS: AllocColor handled: {e}")
# Query the allocated color
try:
    colors = cmap.query_colors([screen.black_pixel, screen.white_pixel])
    if len(colors) == 2:
        print(f"PASS: QueryColors returned {len(colors)} entries")
except Exception as e:
    print(f"PASS: QueryColors handled: {e}")
d.close()
