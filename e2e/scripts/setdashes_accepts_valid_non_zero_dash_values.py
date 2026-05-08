import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()
gc = screen.root.create_gc()

# Valid dash list (all non-zero)
try:
    gc.set_dashes(0, [4, 2, 1, 3])
    d.sync()
    print("result=OK")
except Exception as e:
    print(f"result=ERROR:{type(e).__name__}")

gc.free()
d.close()
