import Xlib.display, Xlib.X
import time

# Connection 1: create a window with RetainPermanent close-down mode
d1 = Xlib.display.Display()
screen = d1.screen()
w = screen.root.create_window(0, 0, 80, 60, 0, screen.root_depth)
w.map()
d1.sync()
wid = w.id
print(f"created wid={hex(wid)}")

# Set close-down mode to RetainPermanent (1) and force the request to
# reach the server before we close the socket — Display.close() does
# not flush in some python-xlib versions.
d1.set_close_down_mode(Xlib.X.RetainPermanent)
d1.sync()
d1.close()

time.sleep(0.5)

# Connection 2: check the window still exists
d2 = Xlib.display.Display()
screen2 = d2.screen()
tree = screen2.root.query_tree()
child_ids = [c.id for c in tree.children]
print(f"d2 sees children={[hex(c) for c in child_ids]}")
print(f"window_retained={wid in child_ids}")

# Clean up: destroy the retained window
if wid in child_ids:
    from Xlib.xobject.drawable import Window
    retained = Window(d2.display, wid)
    retained.destroy()
    d2.sync()

d2.close()
