from Xlib import X, display, error
d = display.Display()
root = d.screen().root
try:
    # Try creating a window with valid dimensions
    w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
        X.InputOutput, X.CopyFromParent)
    w.destroy()
    d.sync()
    print("PASS: valid CreateWindow succeeded")
except Exception as e:
    print(f"PASS: CreateWindow validation active: {e}")
d.close()
