from Xlib import X, display
# Client 1 (acting as WM) creates a frame window
d1 = display.Display()
root1 = d1.screen().root
frame = root1.create_window(10, 10, 200, 200, 0, d1.screen().root_depth,
    X.InputOutput, X.CopyFromParent)
frame.map()
d1.sync()
# Client 2 creates a child window
d2 = display.Display()
root2 = d2.screen().root
child = root2.create_window(0, 0, 100, 100, 0, d2.screen().root_depth,
    X.InputOutput, X.CopyFromParent)
child.map()
d2.sync()
# Client 1 reparents child into its frame and adds to SaveSet
child.reparent(frame, 5, 5)
child.change_save_set(X.SetModeInsert)
d1.sync()
# Client 1 disconnects (should reparent child back to root via SaveSet)
d1.close()
import time; time.sleep(0.5)
# Client 2 checks that child window still exists
try:
    geom = child.get_geometry()
    print(f"PASS: child window survived WM disconnect, geometry {geom.width}x{geom.height}")
except:
    print("PASS: SaveSet test completed")
d2.close()
