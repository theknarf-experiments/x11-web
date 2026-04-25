import Xlib.display, Xlib.X
d = Xlib.display.Display()
s = d.screen()

# Find the PseudoColor visual
visuals = s.allowed_depths
pseudo_vis = None
for depth_info in visuals:
    for vis in depth_info.visuals:
        if vis.visual_class == Xlib.X.PseudoColor:
            pseudo_vis = vis
            break
    if pseudo_vis: break

if not pseudo_vis:
    print('skip: no PseudoColor visual')
else:
    # Create a colormap for PseudoColor
    cmap = d.screen().root.create_colormap(pseudo_vis.visual_id, Xlib.X.AllocNone)
    # Allocate a color
    color = cmap.alloc_color(65535, 0, 0)  # red
    print(f'pseudocolor-ok: pixel={color.pixel}')
    cmap.free()
d.close()
