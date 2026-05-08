import Xlib.display
import Xlib.X
d = Xlib.display.Display()
screen = d.screen()
root = screen.root

# Warp pointer to a specific location (absolute, relative to root)
root.warp_pointer(100, 200)
d.sync()

# Query pointer to verify position
qp = root.query_pointer()
print(f"x_after_warp={qp.root_x}")
print(f"y_after_warp={qp.root_y}")
print("warp=ok")
d.close()
