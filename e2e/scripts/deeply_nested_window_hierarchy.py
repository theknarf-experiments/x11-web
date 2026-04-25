import Xlib.display, Xlib.X
d = Xlib.display.Display(":99")
screen = d.screen()
root = screen.root
DEPTH = 20
windows = [root]
for i in range(DEPTH):
    parent = windows[-1]
    w = parent.create_window(1, 1, max(100 - i*4, 10), max(100 - i*4, 10), 0,
        screen.root_depth)
    w.map()
    windows.append(w)
d.sync()
# Verify deepest window geometry
geom = windows[-1].get_geometry()
print(f"DEEPEST_SIZE={geom.width}x{geom.height}")
# TranslateCoordinates from deepest to root
tc = d.screen().root.translate_coords(windows[-1], 0, 0)
print(f"TRANSLATE={tc.x},{tc.y}")
# Cleanup - destroy from bottom up
for w in reversed(windows[1:]):
    w.destroy()
d.sync()
print("NESTED_WINDOWS_OK")
d.close()
