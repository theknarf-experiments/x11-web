import Xlib.display
# Open 5 simultaneous connections
displays = []
for i in range(5):
    d = Xlib.display.Display(":99")
    displays.append(d)
# Each connection should independently work
for i, d in enumerate(displays):
    root = d.screen().root
    w = root.create_window(i * 10, 0, 50, 50, 0, d.screen().root_depth)
    w.map()
    d.sync()
    geom = w.get_geometry()
    assert geom.width == 50, f"Conn {i}: width mismatch"
    w.destroy()
    d.sync()
# Close all connections
for d in displays:
    d.close()
print("MULTI_CONN_OK")
