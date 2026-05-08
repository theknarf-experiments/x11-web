from Xlib import display, X
d = display.Display()
# python-xlib in the sidecar doesn't ship Xlib.ext.xkb, so skip
# d.xkb_get_controls and just probe the extension presence.
ext = d.query_extension('XKEYBOARD')
print(f"xkb_present={'true' if ext else 'false'}")
d.close()
