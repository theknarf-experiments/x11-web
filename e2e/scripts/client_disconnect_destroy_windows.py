import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
# Client 1: create a window then disconnect. depth=CopyFromParent
# matches the root depth without us having to query it explicitly.
d1 = Xlib.display.Display()
root = d1.screen().root
w1 = root.create_window(0, 0, 100, 100, 0, Xlib.X.CopyFromParent, Xlib.X.InputOutput)
w1.map()
wid = w1.id
d1.sync()
passed += 1; print(f"PASS: client1 created window {wid:#x}")
# Display.close() shuts down the unix socket; the server's read loop
# sees n==0 (or a write-side EPIPE on a pending event flush) and runs
# the close-down-mode cleanup, which removes the window from the
# shared registry.
d1.close()
import time; time.sleep(0.5)
# Client 2: confirm the window is gone from the shared registry.
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
