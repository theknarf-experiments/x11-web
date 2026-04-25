import Xlib.display, Xlib.X
import threading, time

# First connection grabs the server
d1 = Xlib.display.Display()
d1.grab_server()
d1.sync()

grabbed = True
d2_result = [None]

def try_second_connection():
    try:
        d2 = Xlib.display.Display()
        # This should block while server is grabbed
        s = d2.screen()
        root = s.root
        # Try a simple request
        g = root.get_geometry()
        d2_result[0] = 'completed'
        d2.close()
    except Exception as e:
        d2_result[0] = f'error: {e}'

t = threading.Thread(target=try_second_connection)
t.start()

# Give the second connection a chance to start
time.sleep(0.3)

# Ungrab and let the second connection proceed
d1.ungrab_server()
d1.sync()

# Wait for the second connection to complete
t.join(timeout=5)
print(f'grab-test: d2={d2_result[0]}')
d1.close()
