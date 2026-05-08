from Xlib import display
d = display.Display()
# Check XKB extension via raw query (the python-xlib build in the
# sidecar doesn't ship Xlib.ext.xkb)
try:
    xkb_info = d.query_extension("XKEYBOARD")
    if xkb_info:
        print("xkb_present=True")
    else:
        print("xkb_present=False")
except:
    print("xkb_present=False")
d.close()
