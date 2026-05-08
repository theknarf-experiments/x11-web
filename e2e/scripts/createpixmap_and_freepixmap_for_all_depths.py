import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

depths_ok = []
for depth in [1, 4, 8, 16, 24, 32]:
    try:
        pm = screen.root.create_pixmap(10, 10, depth)
        pm.free()
        depths_ok.append(depth)
    except Exception as e:
        print(f"depth_{depth}_error={e}")

print(f"depths={depths_ok}")
d.close()
