from Xlib import X, display
d = display.Display()
root = d.screen().root
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    X.InputOutput, X.CopyFromParent,
    backing_store=X.Always)
attrs = w.get_attributes()
print(f'backing_store={attrs.backing_store}')
w.destroy()
d.close()
