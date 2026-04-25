from Xlib import display, X
d = display.Display()
info = d.info
# Verify protocol version
assert info.protocol_major_version == 11, f"Bad major: {info.protocol_major_version}"
assert info.protocol_minor_version == 0, f"Bad minor: {info.protocol_minor_version}"
# Verify screen info
assert info.roots and len(info.roots) >= 1, "No screens"
screen = info.roots[0]
assert screen.width_in_pixels > 0, "Zero width"
assert screen.height_in_pixels > 0, "Zero height"
assert screen.root_depth >= 24, f"Low depth: {screen.root_depth}"
print(f"PASS: X11.{info.protocol_major_version} vendor={info.vendor} screen={screen.width_in_pixels}x{screen.height_in_pixels}x{screen.root_depth}")
d.close()
