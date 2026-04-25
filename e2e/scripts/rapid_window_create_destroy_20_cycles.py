from Xlib import X, display
d = display.Display()
root = d.screen().root
for i in range(20):
    w = root.create_window(0, 0, 100+i, 100+i, 0, d.screen().root_depth)
    w.map()
    d.sync()
    w.destroy()
    d.sync()
print('rapid-create-destroy-ok')
d.close()
