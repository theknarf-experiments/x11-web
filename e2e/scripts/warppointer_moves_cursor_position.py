import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Warp pointer to specific location (absolute coords on root —
# Display.warp_pointer signature is (x, y, src_window=0, ...))
d.warp_pointer(500, 300)
d.sync()

ptr = screen.root.query_pointer()
print(f"x={ptr.root_x} y={ptr.root_y}")
print(f"warp_ok={ptr.root_x == 500 and ptr.root_y == 300}")

d.close()
