from Xlib import display, X
d = display.Display()
# python-xlib stores connection-setup fields on d.display.info under
# the names protocol_major / protocol_minor (not the Xlib C-style
# protocol_major_version / protocol_minor_version).
info_data = d.display.info._data
major = info_data["protocol_major"]
minor = info_data["protocol_minor"]
assert major == 11, f"Bad major: {major}"
assert minor == 0, f"Bad minor: {minor}"
screen = d.screen()
assert screen.width_in_pixels > 0, "Zero width"
assert screen.height_in_pixels > 0, "Zero height"
assert screen.root_depth >= 24, f"Low depth: {screen.root_depth}"
vendor = info_data["vendor"]
print(f"PASS: X11.{major} vendor={vendor} screen={screen.width_in_pixels}x{screen.height_in_pixels}x{screen.root_depth}")
d.close()
