import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Alloc a specific color
cm = screen.default_colormap
color = cm.alloc_color(0xFFFF, 0x0000, 0x8080)
print(f"pixel={color.pixel}")
print(f"red={color.red}")
print(f"green={color.green}")
print(f"blue={color.blue}")
# Red channel should be 0xFFFF, green 0, blue ~0x8080
print(f"red_match={color.red == 0xFFFF}")

d.close()
