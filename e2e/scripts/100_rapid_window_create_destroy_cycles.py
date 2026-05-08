from Xlib import X, display
d = display.Display()
root = d.screen().root
count = 100
for i in range(count):
    w = root.create_window(0, 0, 50, 50, 0, d.screen().root_depth)
    w.map()
    d.sync()
    w.destroy()
    d.sync()
print(f"completed={count}")
d.close()
