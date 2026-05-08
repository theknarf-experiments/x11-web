import Xlib.display, Xlib.X
d = Xlib.display.Display()
root = d.screen().root
w = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Test valid stack modes: Above(0), Below(1), TopIf(2), BottomIf(3), Opposite(4)
results = []
for mode in range(5):
    try:
        w.configure(stack_mode=mode)
        d.sync()
        results.append(f"MODE_{mode}_OK")
    except Exception as e:
        results.append(f"MODE_{mode}_FAIL:{e}")

print(" ".join(results))
w.destroy()
d.sync()
