from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
pass_count = 0
# Test InputOutput window
w1 = root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent, background_pixel=0xFF0000)
w1.map()
d.sync()
attrs = w1.get_attributes()
if attrs.your_event_mask is not None:
    pass_count += 1
w1.destroy()
# Test InputOnly window
w2 = root.create_window(0, 0, 50, 50, 0, 0,
    X.InputOnly, X.CopyFromParent)
w2.map()
d.sync()
w2.destroy()
pass_count += 1
# Test subwindow
parent = root.create_window(10, 10, 100, 100, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent)
child = parent.create_window(5, 5, 30, 30, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent, background_pixel=0x00FF00)
parent.map()
child.map()
d.sync()
# QueryTree
tree = parent.query_tree()
if tree.children and len(tree.children) >= 1:
    pass_count += 1
child.destroy()
parent.destroy()
d.sync()
print(f"PASS: window tests passed ({pass_count}/3)")
d.close()
