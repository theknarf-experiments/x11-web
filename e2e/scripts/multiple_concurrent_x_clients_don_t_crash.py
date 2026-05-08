import Xlib.display
import threading
import time

results = []
errors = []

def create_window(idx):
    try:
        d = Xlib.display.Display()
        screen = d.screen()
        w = screen.root.create_window(
            10 + idx * 20, 10, 50, 50, 0, screen.root_depth)
        w.map()
        d.sync()
        time.sleep(0.3)
        w.destroy()
        d.sync()
        d.close()
        results.append(f"client_{idx}_ok")
    except Exception as e:
        errors.append(f"client_{idx}_error={e}")

threads = []
for i in range(5):
    t = threading.Thread(target=create_window, args=(i,))
    threads.append(t)
    t.start()

for t in threads:
    t.join()

print(f"ok_count={len(results)}")
print(f"error_count={len(errors)}")
for e in errors:
    print(e)
