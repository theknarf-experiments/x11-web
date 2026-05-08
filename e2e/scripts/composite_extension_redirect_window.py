import Xlib.display, Xlib.X

d = Xlib.display.Display()
composite_info = d.query_extension('Composite')
if composite_info and composite_info.present:
    print("COMPOSITE_PRESENT")
else:
    print("COMPOSITE_MISSING")
d.close()
