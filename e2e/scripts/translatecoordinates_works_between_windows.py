import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w1 = screen.root.create_window(100, 200, 50, 50, 0, screen.root_depth)
w1.map()
d.sync()

# Translate (0,0) in w1 to root coordinates
result = w1.translate_coords(screen.root, 0, 0)
# Should be approximately (100, 200)
print(f"translated_x={result.x}")
print(f"translated_y={result.y}")
print(f"translate_ok=True")

w1.destroy()
d.close()
