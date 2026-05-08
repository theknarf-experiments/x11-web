import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Set _NET_WM_NAME (UTF-8)
net_wm_name = d.intern_atom('_NET_WM_NAME')
utf8_string = d.intern_atom('UTF8_STRING')
title = 'Test Window — Ünïcödé ✓'
w.change_property(net_wm_name, utf8_string, 8, title.encode('utf-8'))
d.sync()

# Read it back
prop = w.get_full_property(net_wm_name, utf8_string)
if prop and prop.value.decode('utf-8') == title:
    print("UTF8_TITLE_OK")
else:
    print(f"FAIL: got {prop.value if prop else None}")

d.close()
