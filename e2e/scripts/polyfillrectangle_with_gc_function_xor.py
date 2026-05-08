import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# Fill with white
gc_white = w.create_gc(foreground=0xFFFFFF, function=Xlib.X.GXcopy)
w.fill_rectangle(gc_white, 0, 0, 100, 100)
d.sync()

# XOR with red
gc_xor = w.create_gc(foreground=0xFF0000, function=Xlib.X.GXxor)
w.fill_rectangle(gc_xor, 10, 10, 50, 50)
d.sync()

# Get the pixel at (20, 20) - should be white XOR red = cyan (0x00FFFF)
img = w.get_image(20, 20, 1, 1, Xlib.X.ZPixmap, 0xFFFFFFFF)
import struct
pixel = struct.unpack('<I', img.data[:4])[0] & 0xFFFFFF
print(f"pixel=0x{pixel:06x}")
# white (0xFFFFFF) XOR red (0xFF0000) = cyan (0x00FFFF)
print(f"xor_correct={pixel == 0x00FFFF}")

w.destroy()
d.close()
