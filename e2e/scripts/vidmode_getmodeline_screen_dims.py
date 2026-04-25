import Xlib, Xlib.display
d = Xlib.display.Display()
root = d.screen().root
w = root.get_geometry().width
h = root.get_geometry().height
print(f'screen-dimensions: {w}x{h}')
if w > 0 and h > 0: print('vidmode-dimensions-ok')
