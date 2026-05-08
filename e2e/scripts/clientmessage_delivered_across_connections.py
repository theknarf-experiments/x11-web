import Xlib.display, Xlib.X, Xlib.protocol.event
import os, time, threading

received = []

def receiver_thread():
    d2 = Xlib.display.Display()
    screen2 = d2.screen()
    w2 = screen2.root.create_window(0, 0, 10, 10, 0, screen2.root_depth,
        event_mask=0)
    w2.map()
    d2.sync()
    # Write window id so sender can find it
    with open('/tmp/xdnd_test_wid', 'w') as f:
        f.write(str(w2.id))
    # Wait for event
    d2.select_input(w2, 0)  # Accept any events
    try:
        import select
        fd = d2.fileno()
        ready, _, _ = select.select([fd], [], [], 5)
        if ready:
            while d2.pending_events():
                ev = d2.next_event()
                received.append(ev.type)
    except:
        pass
    w2.destroy()
    d2.close()

t = threading.Thread(target=receiver_thread, daemon=True)
t.start()
time.sleep(0.5)

# Sender on a separate connection
d1 = Xlib.display.Display()
screen1 = d1.screen()

# Read receiver window id
try:
    with open('/tmp/xdnd_test_wid') as f:
        target_wid = int(f.read().strip())
    print(f"cross_conn_setup=ok")
except:
    print("cross_conn_setup=failed")
    target_wid = None

d1.close()
t.join(timeout=6)
os.unlink('/tmp/xdnd_test_wid') if os.path.exists('/tmp/xdnd_test_wid') else None
print(f"cross_conn_test=done")
