import Xlib.display, Xlib.X
d = Xlib.display.Display(":99")
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 200, 200, 0, screen.root_depth,
    override_redirect=True,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.ExposureMask)
w.map()
d.sync()
# Resize
w.configure(width=300, height=250, x=50, y=75)
d.sync()
geom = w.get_geometry()
print(f"NEW_GEOM={geom.x},{geom.y},{geom.width},{geom.height}")
assert geom.width == 300, f"width={geom.width}"
assert geom.height == 250, f"height={geom.height}"
# Change stacking (raise)
w.configure(stack_mode=Xlib.X.Above)
d.sync()
w.destroy()
d.sync()
print("CONFIGURE_OK")
d.close()
