import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
ptr = screen.root.query_pointer()
print(f"root_x={ptr.root_x} root_y={ptr.root_y}")
print(f"same_screen={ptr.same_screen}")
d.close()
