import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

depths_ok = []
for depth in [1, 8, 24, 32]:
    try:
        pm = screen.root.create_pixmap(100, 100, depth)
        pm.free()
        depths_ok.append(depth)
    except Exception as e:
        print(f"depth {depth}: {e}")

print(f"depths_ok={depths_ok}")
if 1 in depths_ok and 24 in depths_ok:
    print("PIXMAP_DEPTHS_OK")

d.close()
