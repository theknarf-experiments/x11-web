import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.ButtonPressMask)
w.map()
d.sync()

errors = 0
for i in range(100):
    try:
        d.grab_server()
        d.sync()
        d.ungrab_server()
        d.sync()
    except Exception:
        errors += 1

print(f"errors={errors}")
print(f"result={'OK' if errors == 0 else 'FAIL'}")

w.destroy()
d.close()
