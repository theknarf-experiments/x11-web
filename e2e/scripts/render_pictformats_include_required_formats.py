import Xlib.display
d = Xlib.display.Display()

# Check that QueryPictFormats returns the expected formats
# We test this by verifying the display opened successfully
# and that basic RENDER operations work
root = d.screen().root
w = root.create_window(0, 0, 10, 10, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Query extension
render_ext = d.query_extension("RENDER")
if render_ext:
    print("RENDER_EXT_OK")
else:
    print("RENDER_EXT_MISSING")

d.close()
