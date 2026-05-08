import Xlib.display
d = Xlib.display.Display()
exts = d.list_extensions()
print(f"count={len(exts)}")
for e in sorted(exts):
    print(e.decode() if isinstance(e, bytes) else e)
d.close()
