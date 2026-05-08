import Xlib.display
d = Xlib.display.Display()
exts = d.list_extensions()
for e in sorted(exts):
    print(e)
d.close()
