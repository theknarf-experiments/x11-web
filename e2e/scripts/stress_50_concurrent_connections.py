import Xlib.display, Xlib.X
import threading

results = []

def connect_and_query(idx):
    try:
        d = Xlib.display.Display()
        s = d.screen()
        root = s.root
        g = root.get_geometry()
        # Create a window, map it, destroy it
        w = root.create_window(
            idx * 2, idx * 2, 50, 50, 0,
            s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
        )
        w.map()
        d.sync()
        w.destroy()
        d.close()
        results.append('ok')
    except Exception as e:
        results.append(f'err:{e}')

threads = []
for i in range(50):
    t = threading.Thread(target=connect_and_query, args=(i,))
    threads.append(t)
    t.start()

for t in threads:
    t.join(timeout=30)

ok_count = sum(1 for r in results if r == 'ok')
err_count = len(results) - ok_count
print(f'stress-50: ok={ok_count} err={err_count}')
