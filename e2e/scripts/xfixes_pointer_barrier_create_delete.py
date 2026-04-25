import ctypes, ctypes.util
x11 = ctypes.CDLL(ctypes.util.find_library('X11'))
xfixes = ctypes.CDLL(ctypes.util.find_library('Xfixes'))
x11.XOpenDisplay.restype = ctypes.c_void_p
d = x11.XOpenDisplay(b':99')
assert d, 'Failed to open display'
# Query XFixes version (>= 5.0 for barriers)
major = ctypes.c_int(0)
minor = ctypes.c_int(0)
xfixes.XFixesQueryVersion(d, ctypes.byref(major), ctypes.byref(minor))
print(f'XFixes version={major.value}.{minor.value}')
assert major.value >= 5, f'Need XFixes >= 5, got {major.value}'
# Create a barrier at y=100 spanning x=0..800
root = ctypes.c_ulong(x11.XDefaultRootWindow(d))
xfixes.XFixesCreatePointerBarrier.restype = ctypes.c_ulong
# XFixesCreatePointerBarrier(dpy, window, x1, y1, x2, y2, directions, num_devices, devices)
barrier = xfixes.XFixesCreatePointerBarrier(d, root, 0, 100, 800, 100, 0, 0, None)
print(f'barrier-id={barrier}')
assert barrier != 0, 'CreatePointerBarrier returned 0'
# Delete the barrier
xfixes.XFixesDestroyPointerBarrier(d, barrier)
x11.XSync(d, 0)
print('PASS: pointer barrier create/delete succeeded')
x11.XCloseDisplay(d)
