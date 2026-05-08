import Xlib.display
import Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create a test window
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    window_class=Xlib.X.InputOutput,
    visual=Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()

# Query window attributes
attrs = w.get_attributes()
print(f"window_class={attrs.your_event_mask}")
print(f"window_exists=true")

geo = w.get_geometry()
print(f"width={geo.width}")
print(f"height={geo.height}")

w.destroy()
d.sync()
print("region_test=ok")
d.close()
