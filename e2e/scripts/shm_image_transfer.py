from Xlib import X, display, Xutil
d = display.Display()
# Check if MIT-SHM is available
ext = d.query_extension('MIT-SHM')
if ext and ext.present:
    print('shm-extension-present')
else:
    print('shm-extension-missing')
d.close()
