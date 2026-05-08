import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(50, 100, 200, 150, 0, screen.root_depth)
w.map()
d.sync()

# Translate (0,0) of window to root coordinates
result = d.screen().root.translate_coords(w, 0, 0)
print(f"x={result.x} y={result.y}")

w.destroy()
d.close()
