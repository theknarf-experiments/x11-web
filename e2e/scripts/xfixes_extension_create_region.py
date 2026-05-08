import Xlib.display

d = Xlib.display.Display()
xfixes_info = d.query_extension('XFIXES')
if xfixes_info and xfixes_info.present:
    print("XFIXES_PRESENT")
else:
    print("XFIXES_MISSING")
d.close()
