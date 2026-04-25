import Xlib.display, Xlib.X, sys, threading
errors = []
def client_work(n):
    try:
        d = Xlib.display.Display()
        root = d.screen().root
        for i in range(10):
            w = root.create_window(n*50, 0, 100, 100, 0, d.screen().root_depth)
            w.map()
            d.sync()
            g = w.get_geometry()
            assert g.width == 100
            w.destroy()
            d.sync()
        d.close()
    except Exception as e:
        errors.append(f"client {n}: {e}")
threads = [threading.Thread(target=client_work, args=(i,)) for i in range(5)]
for t in threads: t.start()
for t in threads: t.join(timeout=30)
if errors:
    print(f"multi-client-errors: {errors}")
    sys.exit(1)
print("multi-client-ok")
