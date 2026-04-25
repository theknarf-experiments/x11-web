import Xlib.display, Xlib.X
d = Xlib.display.Display()
s = d.screen()
root = s.root

# Create parent selecting ButtonPress
parent = root.create_window(
    0, 0, 200, 200, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ButtonPressMask | Xlib.X.StructureNotifyMask,
)
parent.map()
d.sync()

# Create child with do_not_propagate_mask including ButtonPress
child = parent.create_window(
    10, 10, 50, 50, 0,
    s.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
)
child.change_attributes(do_not_propagate_mask=Xlib.X.ButtonPressMask)
child.map()
d.sync()

print('dnp-mask-ok: created windows with do_not_propagate_mask')

child.destroy()
parent.destroy()
d.close()
