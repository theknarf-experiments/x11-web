import Xlib.display

d = Xlib.display.Display()
xtest_info = d.query_extension('XTEST')
if xtest_info and xtest_info.present:
    print("XTEST_PRESENT")
else:
    print("XTEST_MISSING")
d.close()
