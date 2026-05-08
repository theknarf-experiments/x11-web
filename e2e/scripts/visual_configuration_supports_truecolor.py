import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
# Check that root visual is TrueColor (class 4)
print(f"root_depth={screen.root_depth}")
# screen.root_visual may be an int (visual ID) in some python3-xlib versions
# Look up the visual info from the screen's allowed_depths
visual_class = None
for depth_info in screen.allowed_depths:
    for vis in depth_info.visuals:
        if vis.visual_id == screen.root_visual:
            visual_class = vis.visual_class
            break
if visual_class is None and hasattr(screen.root_visual, 'visual_class'):
    visual_class = screen.root_visual.visual_class
print(f"root_visual_class={visual_class}")
# TrueColor = 4
print(f"is_truecolor={visual_class == 4}")
d.close()
