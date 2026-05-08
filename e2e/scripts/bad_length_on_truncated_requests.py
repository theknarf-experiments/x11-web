import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()

# Create a valid window first
w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth)
d.sync()

# The server should handle malformed requests gracefully
# (not crash). Just verify the connection is still alive.
geo = w.get_geometry()
print(f"still_alive={geo.width == 50}")

w.destroy()
d.close()
