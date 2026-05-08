import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

created = 0
destroyed = 0
errors = 0

for i in range(100):
    try:
        w = screen.root.create_window(
            i % 20 * 50, i // 20 * 50, 40, 40, 0,
            screen.root_depth,
            event_mask=Xlib.X.ExposureMask,
        )
        w.map()
        d.sync()
        created += 1
        w.destroy()
        d.sync()
        destroyed += 1
    except Exception as e:
        errors += 1

print(f"created={created}")
print(f"destroyed={destroyed}")
print(f"errors={errors}")
print(f"success={created == 100 and destroyed == 100 and errors == 0}")
d.close()
