import Xlib.display
import Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 4, 4, 0, screen.root_depth,
    window_class=Xlib.X.InputOutput,
    visual=Xlib.X.CopyFromParent)
w.map()
d.sync()

gc = w.create_gc()

# Create a known pixel pattern: 4x4 red pixels
import struct
pixels = b''
for y in range(4):
    for x in range(4):
        pixels += struct.pack('BBBB', 0, 0, 255, 0)  # BGRA = red

w.put_image(gc, 0, 0, 4, 4, Xlib.X.ZPixmap, 24, 0, pixels)
d.sync()

# Read back
img = w.get_image(0, 0, 4, 4, Xlib.X.ZPixmap, 0xFFFFFFFF)
raw = img.data
print(f"image_len={len(raw)}")
# Check first pixel is red (B=0, G=0, R=255)
if len(raw) >= 4:
    b, g, r = raw[0], raw[1], raw[2]
    print(f"pixel_r={r}")
    print(f"pixel_g={g}")
    print(f"pixel_b={b}")
    print(f"red_match={r == 255 and g == 0 and b == 0}")

w.destroy()
d.sync()
print("putget_test=ok")
d.close()
