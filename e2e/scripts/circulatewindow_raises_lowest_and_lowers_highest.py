import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

parent = screen.root.create_window(0, 0, 300, 300, 0, screen.root_depth)
w1 = parent.create_window(0, 0, 100, 100, 0, screen.root_depth)
w2 = parent.create_window(0, 0, 100, 100, 0, screen.root_depth)
w1.map()
w2.map()
parent.map()
d.sync()

# CirculateRaiseLowest (direction=0)
parent.circulate(Xlib.X.RaiseLowest)
d.sync()

tree = parent.query_tree()
ids = [c.id for c in tree.children]
print(f"after_raise_lowest_top={ids[-1] == w1.id}")

# CirculateLowerHighest (direction=1)
parent.circulate(Xlib.X.LowerHighest)
d.sync()

tree2 = parent.query_tree()
ids2 = [c.id for c in tree2.children]
print(f"after_lower_highest_bottom={ids2[0] == w1.id}")

w1.destroy()
w2.destroy()
parent.destroy()
d.close()
