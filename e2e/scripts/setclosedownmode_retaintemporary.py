import Xlib.display, Xlib.X, sys, struct
passed = 0; failed = 0
d1 = Xlib.display.Display()
root = d1.screen().root
w1 = root.create_window(0, 0, 100, 100, 0, 24, Xlib.X.InputOutput)
w1.map()
wid = w1.id
d1.sync()
# SetCloseDownMode to RetainTemporary (2)
d1.set_close_down_mode(Xlib.X.RetainTemporary)
d1.sync()
passed += 1; print(f"PASS: set RetainTemporary, window={wid:#x}")
d1.close()
import time; time.sleep(0.5)
d2 = Xlib.display.Display()
root2 = d2.screen().root
tree = root2.query_tree()
child_ids = [c.id for c in tree.children]
if wid in child_ids:
    passed += 1; print(f"PASS: window {wid:#x} retained after disconnect")
else:
    failed += 1; print(f"FAIL: window {wid:#x} not retained")
# Clean up: KillClient to destroy retained resources
# (kill_client is on the Resource object, not Display)
d2.create_resource_object("window", wid).kill_client()
d2.sync()
passed += 1; print("PASS: KillClient on retained window")
d2.close()
print(f"cleanup-retain: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
