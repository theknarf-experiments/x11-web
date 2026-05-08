import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
root = screen.root

# Root depth (24-bit TrueColor)
w24 = root.create_window(0, 0, 10, 10, 0, 24, Xlib.X.InputOutput,
                          Xlib.X.CopyFromParent)
geo = w24.get_geometry()
print(f"depth_24={geo.depth}")

# Try creating with depth 32 (ARGB visual 0x40)
try:
    visual_argb = None
    for depth_info in screen.allowed_depths:
        if depth_info.depth == 32:
            for v in depth_info.visuals:
                visual_argb = v.visual_id
                break
    if visual_argb:
        w32 = root.create_window(0, 0, 10, 10, 0, 32, Xlib.X.InputOutput,
                                  visual_argb)
        geo32 = w32.get_geometry()
        print(f"depth_32={geo32.depth}")
        w32.destroy()
    else:
        print("depth_32=no_visual")
except Exception as e:
    print(f"depth_32=error:{e}")

w24.destroy()
d.close()
