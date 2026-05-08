import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

parent1 = screen.root.create_window(0, 0, 200, 200, 0, screen.root_depth)
parent2 = screen.root.create_window(200, 0, 200, 200, 0, screen.root_depth)
child = parent1.create_window(10, 10, 50, 50, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)

# Map both parents first so the child can become IsViewable.
parent1.map()
parent2.map()
child.map()
d.sync()

# Check it's mapped
attrs = child.get_attributes()
print(f"before_map_state={attrs.map_state}")

# Reparent the mapped window to parent2
child.reparent(parent2, 20, 20)
d.sync()

# It should still be mapped after reparent
attrs2 = child.get_attributes()
print(f"after_map_state={attrs2.map_state}")

child.destroy()
parent1.destroy()
parent2.destroy()
d.close()
