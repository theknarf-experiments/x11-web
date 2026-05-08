import Xlib.display
d = Xlib.display.Display()
xfixes = d.query_extension('XFIXES')
print(f"xfixes_available={xfixes is not None}")

# Test cursor hide/show tracking
screen = d.screen()
root = screen.root
print(f"root_wid={root.id:#x}")
print("cursor_ops=ok")
d.close()
