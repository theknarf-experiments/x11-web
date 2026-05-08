from Xlib import X, display
d = display.Display()
root = d.screen().root

# Create InputOnly window (class=2, depth=0)
w = root.create_window(0, 0, 100, 100, 0, 0,
    window_class=X.InputOnly)
w.map()
d.sync()

attrs = w.get_attributes()
print(f"class={attrs.win_class}")
print(f"map_state={attrs.map_state}")

# GetGeometry should still work
geom = w.get_geometry()
print(f"width={geom.width} height={geom.height}")

w.destroy()
d.close()
