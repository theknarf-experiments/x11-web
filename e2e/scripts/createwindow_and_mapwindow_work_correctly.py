import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
w = screen.root.create_window(
    10, 10, 100, 50, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask | Xlib.X.StructureNotifyMask,
)
w.map()
d.sync()
# Query the window geometry
geo = w.get_geometry()
print(f"width={geo.width} height={geo.height}")
# Query attributes
attrs = w.get_attributes()
print(f"map_state={attrs.map_state}")
w.destroy()
d.close()
