from Xlib import X, display
d = display.Display()
screen = d.screen()
cmap = screen.default_colormap
color = cmap.alloc_color(65535, 0, 0)
print(f'pixel={color.pixel}')
d.close()
