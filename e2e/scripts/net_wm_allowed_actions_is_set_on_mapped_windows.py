import Xlib.display, Xlib.X
import time
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth)
w.map()
d.sync()
time.sleep(0.2)

allowed_atom = d.intern_atom('_NET_WM_ALLOWED_ACTIONS')
prop = w.get_full_property(allowed_atom, d.intern_atom('ATOM'))
if prop and len(prop.value) > 0:
    close_atom = d.intern_atom('_NET_WM_ACTION_CLOSE')
    move_atom = d.intern_atom('_NET_WM_ACTION_MOVE')
    resize_atom = d.intern_atom('_NET_WM_ACTION_RESIZE')
    has_close = close_atom in prop.value
    has_move = move_atom in prop.value
    has_resize = resize_atom in prop.value
    print(f"actions_count={len(prop.value)}")
    print(f"has_close={has_close}")
    print(f"has_move={has_move}")
    print(f"has_resize={has_resize}")
else:
    print("no_allowed_actions")

w.destroy()
d.close()
