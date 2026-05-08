import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Check RENDER extension is available
try:
    render_info = d.query_extension('RENDER')
    if render_info and render_info.present:
        print("RENDER_PRESENT")
    else:
        print("RENDER_MISSING")
except Exception as e:
    print(f"RENDER check: {e}")

d.close()
