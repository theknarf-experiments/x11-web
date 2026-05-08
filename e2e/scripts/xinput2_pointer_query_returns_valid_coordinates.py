import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
root = screen.root

# Query pointer location
qp = root.query_pointer()
print(f"root_x={qp.root_x}")
print(f"root_y={qp.root_y}")
print(f"same_screen={qp.same_screen}")
print(f"mask={qp.mask}")
print("pointer_query=ok")
d.close()
