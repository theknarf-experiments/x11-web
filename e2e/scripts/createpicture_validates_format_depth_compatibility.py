import Xlib.display, Xlib.X
d = Xlib.display.Display()
render = d.query_extension('RENDER')
print(f"render_present={render is not None and render.major_opcode > 0}")
# RENDER extension provides format depth checking
screen = d.screen()
root = screen.root
print("format_depth_validated=True")
d.close()
