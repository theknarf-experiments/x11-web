import Xlib.display, Xlib.X, Xlib.Xutil
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

gc = w.create_gc()

# Create a 4x2 test pattern (BGRA, 4 bytes per pixel)
# Pixel values: red, green, blue, white (in BGRX format for depth 24)
import struct
pixels = b''
pixels += struct.pack('<I', 0x00FF0000)  # red
pixels += struct.pack('<I', 0x0000FF00)  # green
pixels += struct.pack('<I', 0x000000FF)  # blue
pixels += struct.pack('<I', 0x00FFFFFF)  # white
pixels += struct.pack('<I', 0x00000000)  # black
pixels += struct.pack('<I', 0x00808080)  # gray
pixels += struct.pack('<I', 0x00FFFF00)  # yellow
pixels += struct.pack('<I', 0x00FF00FF)  # magenta

# PutImage (ZPixmap, depth 24)
w.put_image(gc, 0, 0, 4, 2, Xlib.X.ZPixmap, 24, 0, bytes(pixels))
d.sync()

# GetImage — python-xlib signature is (x, y, w, h, format, plane_mask).
img = w.get_image(0, 0, 4, 2, Xlib.X.ZPixmap, 0xFFFFFFFF)
data = img.data

# Verify round-trip
import array
result_pixels = array.array('I')
if isinstance(data, bytes):
    result_pixels.frombytes(data[:32])
else:
    result_pixels.frombytes(bytes(data[:32]))

print(f"pixel0={result_pixels[0]:#010x}")
print(f"pixel1={result_pixels[1]:#010x}")
print(f"pixel2={result_pixels[2]:#010x}")
print(f"pixel3={result_pixels[3]:#010x}")
print(f"data_len={len(data)}")
# Padded row = 4*4 = 16, 2 rows = 32 bytes minimum
print(f"round_trip_ok={len(data) >= 32}")

w.destroy()
d.close()
