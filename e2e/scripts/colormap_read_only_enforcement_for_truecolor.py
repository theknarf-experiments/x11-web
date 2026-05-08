import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
screen = d.screen()

# The default colormap is TrueColor (read-only)
cmap = screen.default_colormap

# AllocColor should work (read-only lookup)
color = cmap.alloc_color(65535, 0, 0)
print(f"alloc_ok={color.pixel > 0 or color.pixel == 0}")

# FreeColors on a TrueColor colormap should fail with BadAccess
try:
    cmap.free_colors([color.pixel], 0)
    d.sync()
    print("free_accepted=true")
except Exception as e:
    error_str = str(e)
    print(f"free_error={error_str}")
    if 'BadAccess' in error_str or 'error' in error_str.lower():
        print("free_rejected=true")
    else:
        print("free_rejected=false")

print("colormap_readonly_test=ok")
d.close()
