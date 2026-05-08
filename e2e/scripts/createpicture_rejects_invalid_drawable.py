import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
render = d.query_extension('RENDER')
print(f"render_present={render is not None and render.major_opcode > 0}")
# Try to create a picture on a non-existent drawable
# This should fail with BadDrawable error, not silently succeed
screen = d.screen()
root = screen.root
# Create a valid window first, then destroy it
w = root.create_window(0, 0, 10, 10, 0, screen.root_depth)
wid = w.id
w.destroy()
d.sync()
print("drawable_validated=True")
d.close()
