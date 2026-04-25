from Xlib import display
d = display.Display()
s = d.screen()
print(f'screen: {s.width_in_pixels}x{s.height_in_pixels}')
print(f'root: {s.root.id:#x}')
print(f'depth: {s.root_depth}')
print('PYTHON_XLIB_OK')
d.close()
