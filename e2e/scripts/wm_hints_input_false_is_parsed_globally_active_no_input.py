from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)
w.set_wm_hints(flags=Xutil.InputHint, input=0)
d.sync()

read_hints = w.get_wm_hints()
input_val = getattr(read_hints, 'input', -1) if read_hints else -1
print(f"input={input_val}")
w.destroy()
d.close()
