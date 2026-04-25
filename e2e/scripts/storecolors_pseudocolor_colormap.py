from Xlib import X, display
d = display.Display()
screen = d.screen()
# Find PseudoColor visual
pc_visual = None
for depth_info in screen.allowed_depths:
    for v in depth_info.visuals:
        if v.visual_class == X.PseudoColor:
            pc_visual = v.visual_id
            break
if pc_visual:
    print(f'found PseudoColor visual={pc_visual:#x}')
    cmap = d.create_colormap(screen.root, pc_visual, X.AllocNone)
    color = cmap.alloc_color(0, 65535, 0)
    print(f'alloc_color pixel={color.pixel}')
    cmap.free()
else:
    print('no PseudoColor visual found')
d.close()
