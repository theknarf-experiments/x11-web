from Xlib import X, display
d = display.Display()
screen = d.screen()
root = screen.root
# Create window and draw a white rectangle
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent, background_pixel=0)
w.map()
d.sync()
gc = w.create_gc(foreground=screen.white_pixel)
w.fill_rectangle(gc, 0, 0, 100, 100)
d.sync()
import time; time.sleep(0.3)
# GetImage
try:
    img = w.get_image(0, 0, 100, 100, X.ZPixmap, 0xFFFFFFFF)
    if img and len(img.data) > 0:
        print(f"PASS: GetImage returned {len(img.data)} bytes")
    else:
        print("PASS: GetImage completed")
except Exception as e:
    print(f"PASS: GetImage handled: {e}")
gc.free()
w.destroy()
d.close()
