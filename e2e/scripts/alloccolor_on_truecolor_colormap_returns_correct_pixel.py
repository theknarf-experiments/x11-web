import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# The default colormap is TrueColor
cmap = screen.default_colormap
result = cmap.alloc_color(0xFFFF, 0, 0)  # Red
pixel = result.pixel
r = (pixel >> 16) & 0xFF
# For TrueColor, red should be in the high byte
if r == 0xFF:
    print("ALLOC_COLOR_RED_OK")
else:
    print(f"ALLOC_COLOR_RED_FAIL: pixel={pixel:#x} r={r}")

# Blue
result2 = cmap.alloc_color(0, 0, 0xFFFF)
pixel2 = result2.pixel
b = pixel2 & 0xFF
if b == 0xFF:
    print("ALLOC_COLOR_BLUE_OK")
else:
    print(f"ALLOC_COLOR_BLUE_FAIL: pixel={pixel2:#x} b={b}")
