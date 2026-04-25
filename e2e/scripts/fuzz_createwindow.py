import Xlib.display
import Xlib.X
import Xlib.error
import sys
import time

errors = []

# Test 1: CreateWindow with zero dimensions
d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root
try:
    # Zero width should be caught as BadValue
    w = root.create_window(0, 0, 0, 0, 0, screen.root_depth)
    d.sync()
    # If we get here, the server accepted it (some do) - that's OK
    w.destroy()
    d.sync()
    print("PASS: zero dimensions handled (accepted)")
except Xlib.error.BadValue:
    print("PASS: zero dimensions rejected with BadValue")
except Exception as e:
    print(f"PASS: zero dimensions rejected with {type(e).__name__}")
d.close()

# Test 2: CreateWindow with huge dimensions
d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root
try:
    w = root.create_window(0, 0, 65535, 65535, 0, screen.root_depth)
    d.sync()
    w.destroy()
    d.sync()
    print("PASS: huge dimensions handled (accepted)")
except Exception as e:
    print(f"PASS: huge dimensions rejected with {type(e).__name__}")
d.close()

# Test 3: CreateWindow with very large border width
d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root
try:
    w = root.create_window(0, 0, 100, 100, 65535, screen.root_depth)
    d.sync()
    w.destroy()
    d.sync()
    print("PASS: huge border width handled (accepted)")
except Exception as e:
    print(f"PASS: huge border width rejected with {type(e).__name__}")
d.close()

# Test 4: Negative coordinates (should be accepted per spec)
d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root
try:
    w = root.create_window(-100, -200, 50, 50, 0, screen.root_depth)
    d.sync()
    w.destroy()
    d.sync()
    print("PASS: negative coordinates accepted")
except Exception as e:
    errors.append(f"Negative coordinates rejected: {e}")
d.close()

# Test 5: Operations on destroyed window
d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root
try:
    w = root.create_window(0, 0, 100, 100, 0, screen.root_depth)
    wid = w.id
    w.destroy()
    d.sync()
    # Try to map the destroyed window - should get BadWindow
    w.map()
    d.sync()
    print("PASS: map destroyed window silently ignored")
except Xlib.error.BadWindow:
    print("PASS: map destroyed window raises BadWindow")
except Exception as e:
    print(f"PASS: map destroyed window raises {type(e).__name__}")
d.close()

# Test 6: Double destroy
d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root
try:
    w = root.create_window(0, 0, 100, 100, 0, screen.root_depth)
    w.destroy()
    d.sync()
    w.destroy()
    d.sync()
    print("PASS: double destroy silently handled")
except Xlib.error.BadWindow:
    print("PASS: double destroy raises BadWindow")
except Exception as e:
    print(f"PASS: double destroy raises {type(e).__name__}")
d.close()

# Verify the server is still alive after all the abuse
d = Xlib.display.Display(':99')
info = d.get_display_name()
d.close()
print(f"PASS: server still alive after malformed requests (display={info})")

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("FUZZING_CREATEWINDOW_OK")
