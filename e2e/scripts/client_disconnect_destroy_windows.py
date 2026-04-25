import Xlib.display, Xlib.X, sys, os
passed = 0; failed = 0
# Client 1: create windows and disconnect
d1 = Xlib.display.Display()
root = d1.screen().root
w1 = root.create_window(0, 0, 100, 100, 0, 24, Xlib.X.InputOutput)
w1.map()
wid = w1.id
d1.sync()
passed += 1; print(f"PASS: client1 created window {wid:#x}")
# Close connection (destroys resources in default Destroy mode)
d1.close()
# Client 2: check the window no longer exists
import time; time.sleep(0.5)
d2 = Xlib.display.Display()
root2 = d2.screen().root
tree = root2.query_tree()
child_ids = [c.id for c in tree.children]
if wid not in child_ids:
    passed += 1; print(f"PASS: window {wid:#x} destroyed on disconnect")
else:
    failed += 1; print(f"FAIL: window {wid:#x} still exists after disconnect")
d2.close()
print(f"cleanup-destroy: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
