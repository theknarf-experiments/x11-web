from Xlib import X, display
d = display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth)
w.map()
d.sync()
import time; time.sleep(0.1)

pid_atom = d.intern_atom("_NET_WM_PID")
prop = w.get_property(pid_atom, 0, 0, 100)
if prop and prop.value:
    print(f"has_pid=True pid={prop.value[0]}")
else:
    print("has_pid=False")
w.destroy()
d.close()
