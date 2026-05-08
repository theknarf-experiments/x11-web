import Xlib.display, Xlib.X
d = Xlib.display.Display()
xtest = d.query_extension('XTEST')
print(f"xtest_present={xtest is not None and xtest.major_opcode > 0}")
d.close()
