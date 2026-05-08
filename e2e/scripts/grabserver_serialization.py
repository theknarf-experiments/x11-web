import Xlib.display, Xlib.X
import threading, time, traceback

results = {}

def connection_b_work():
    """Try to do work on connection B while A holds grab."""
    try:
        time.sleep(0.3)  # ensure A has grabbed by now
        start = time.monotonic()
        d2 = Xlib.display.Display()
        # This request should block until A ungrabs
        screen2 = d2.screen()
        _ = screen2.root.query_tree()
        elapsed = time.monotonic() - start
        results["b_elapsed"] = elapsed
        d2.close()
    except Exception as e:
        results["b_error"] = str(e)
        results["b_traceback"] = traceback.format_exc()
        results["b_elapsed"] = time.monotonic() - start

d1 = Xlib.display.Display()
d1.grab_server()
d1.sync()

t = threading.Thread(target=connection_b_work)
t.start()

# Hold the grab for ~1 second
time.sleep(1.0)

d1.ungrab_server()
d1.sync()
t.join(timeout=10)

b_elapsed = results.get("b_elapsed", -1)
b_error = results.get("b_error", "none")
b_tb = results.get("b_traceback", "")
# B should have been blocked for roughly 0.7s (1.0 - 0.3 sleep)
print(f"b_elapsed={b_elapsed:.2f}")
print(f"b_was_blocked={b_elapsed >= 0.5}")
print(f"b_error={b_error}")
if b_tb:
    print(f"b_traceback={b_tb}")
print(f"thread_alive={t.is_alive()}")

d1.close()
