import Xlib.display, Xlib.X, time
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(10, 10, 200, 200, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask | Xlib.X.StructureNotifyMask)
w.map()
d.sync()
time.sleep(0.5)
geom = w.get_geometry()
print(f"mapped_width={geom.width}")
print(f"mapped_height={geom.height}")
w.destroy()
d.sync()
print("lifecycle_ok=True")
d.close()
