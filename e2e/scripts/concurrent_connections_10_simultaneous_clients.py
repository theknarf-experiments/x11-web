import Xlib.display, Xlib.X
import threading

results = {}

def client_work(client_id):
    try:
        d = Xlib.display.Display()
        screen = d.screen()
        w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth)
        w.map()
        d.sync()

        # Do some work
        gc = w.create_gc(foreground=client_id * 0x111111 & 0xFFFFFF)
        w.fill_rectangle(gc, 0, 0, 50, 50)
        d.sync()

        # Read back
        geo = w.get_geometry()
        results[client_id] = geo.width == 50

        w.destroy()
        gc.free()
        d.close()
    except Exception as e:
        results[client_id] = False

threads = []
for i in range(10):
    t = threading.Thread(target=client_work, args=(i,))
    threads.append(t)
    t.start()

for t in threads:
    t.join(timeout=30)

success_count = sum(1 for v in results.values() if v)
print(f"success_count={success_count}")
print(f"total={len(results)}")
print(f"all_ok={success_count == 10}")
