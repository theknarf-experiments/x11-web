import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create parent window
parent = screen.root.create_window(0, 0, 400, 300, 0, screen.root_depth)
parent.map()
d.sync()

# Create transient child
child = screen.root.create_window(50, 50, 200, 150, 0, screen.root_depth)
tf_atom = d.intern_atom('WM_TRANSIENT_FOR')
child.change_property(tf_atom, d.intern_atom('WINDOW'), 32, [parent.id])
child.map()
d.sync()

import time
time.sleep(0.1)

# Query root children to check stacking order
tree = screen.root.query_tree()
children = [c.id for c in tree.children]

if parent.id in children and child.id in children:
    parent_idx = children.index(parent.id)
    child_idx = children.index(child.id)
    if child_idx > parent_idx:
        print("result=OK")
    else:
        print(f"result=WRONG_ORDER,parent_idx={parent_idx},child_idx={child_idx}")
else:
    print("result=NOT_FOUND")

child.destroy()
parent.destroy()
d.close()
