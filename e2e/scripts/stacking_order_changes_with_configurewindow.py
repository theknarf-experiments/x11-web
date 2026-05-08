import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

parent = screen.root.create_window(0, 0, 300, 300, 0, screen.root_depth)
w1 = parent.create_window(0, 0, 100, 100, 0, screen.root_depth)
w2 = parent.create_window(0, 0, 100, 100, 0, screen.root_depth)
w3 = parent.create_window(0, 0, 100, 100, 0, screen.root_depth)
w1.map()
w2.map()
w3.map()
parent.map()
d.sync()

# Initial order should be w1, w2, w3 (bottom to top)
tree = parent.query_tree()
ids = [c.id for c in tree.children]
initial_order = (ids.index(w1.id) < ids.index(w2.id) < ids.index(w3.id))
print(f"initial_order_correct={initial_order}")

# Raise w1 to top (stack_mode=Above=0)
w1.configure(stack_mode=Xlib.X.Above)
d.sync()

tree2 = parent.query_tree()
ids2 = [c.id for c in tree2.children]
# w1 should now be last (topmost)
print(f"w1_on_top={ids2[-1] == w1.id}")

w1.destroy()
w2.destroy()
w3.destroy()
parent.destroy()
d.close()
