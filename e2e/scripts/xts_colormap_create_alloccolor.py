from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Use the default colormap
cmap = screen.default_colormap
# AllocColor: request specific RGB values
reply = cmap.alloc_color(0xFFFF, 0x0000, 0x0000)  # Red
if reply.pixel is not None:
    print(f"PASS: AllocColor returned pixel={reply.pixel:#x} rgb=({reply.red:#06x},{reply.green:#06x},{reply.blue:#06x})")
else:
    print("FAIL: AllocColor returned no pixel")
d.close()
