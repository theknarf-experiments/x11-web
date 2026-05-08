import Xlib.display, Xlib.X

d = Xlib.display.Display()
# Check SHAPE extension
shape_info = d.query_extension('SHAPE')
if shape_info and shape_info.present:
    print("SHAPE_PRESENT")
else:
    print("SHAPE_MISSING")
d.close()
