import Xlib.display
import Xlib.X
import sys

d = Xlib.display.Display(':99')
root = d.screen().root
errors = []

# Create two windows
w1 = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth,
                        window_class=Xlib.X.InputOutput,
                        event_mask=Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask | Xlib.X.PointerMotionMask)
w2 = root.create_window(120, 10, 100, 100, 0, d.screen().root_depth,
                        window_class=Xlib.X.InputOutput,
                        event_mask=Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask | Xlib.X.PointerMotionMask)
w1.map()
w2.map()
d.sync()

# Grab pointer on w1 with owner_events=True
try:
    status = w1.grab_pointer(True,
                             Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask | Xlib.X.PointerMotionMask,
                             Xlib.X.GrabModeAsync,
                             Xlib.X.GrabModeAsync,
                             Xlib.X.NONE,
                             Xlib.X.NONE,
                             Xlib.X.CurrentTime)
    if status == Xlib.X.GrabSuccess:
        print("PASS: GrabPointer(owner_events=True) succeeded")
    else:
        errors.append(f"GrabPointer returned status {status}")
except Exception as e:
    errors.append(f"GrabPointer: {e}")

d.ungrab_pointer(Xlib.X.CurrentTime)

# Grab with owner_events=False
try:
    status = w1.grab_pointer(False,
                             Xlib.X.ButtonPressMask | Xlib.X.ButtonReleaseMask,
                             Xlib.X.GrabModeAsync,
                             Xlib.X.GrabModeAsync,
                             Xlib.X.NONE,
                             Xlib.X.NONE,
                             Xlib.X.CurrentTime)
    if status == Xlib.X.GrabSuccess:
        print("PASS: GrabPointer(owner_events=False) succeeded")
    else:
        errors.append(f"GrabPointer(False) returned status {status}")
except Exception as e:
    errors.append(f"GrabPointer(False): {e}")

d.ungrab_pointer(Xlib.X.CurrentTime)
d.sync()
w1.destroy()
w2.destroy()
d.sync()
d.close()

if errors:
    print(f"FAIL: {errors}")
    sys.exit(1)
print("OWNER_EVENTS_OK")
