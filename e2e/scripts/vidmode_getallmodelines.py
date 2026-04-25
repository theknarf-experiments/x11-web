import ctypes, ctypes.util
lib = ctypes.CDLL(ctypes.util.find_library('Xxf86vm'))
xlib = ctypes.CDLL(ctypes.util.find_library('X11'))
xlib.XOpenDisplay.restype = ctypes.c_void_p
d = xlib.XOpenDisplay(b':99')
assert d, 'Failed to open display'
count = ctypes.c_int(0)
modes = ctypes.c_void_p(0)
# XF86VidModeGetAllModeLines(dpy, screen, count_ptr, modes_ptr)
lib.XF86VidModeGetAllModeLines.restype = ctypes.c_int
ret = lib.XF86VidModeGetAllModeLines(d, 0, ctypes.byref(count), ctypes.byref(modes))
print(f'modes-count={count.value}')
assert count.value >= 1, f'Expected >=1 mode, got {count.value}'
print('PASS: VidMode returned modes')
xlib.XCloseDisplay(d)
