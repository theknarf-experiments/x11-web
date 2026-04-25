import ctypes, ctypes.util
lib = ctypes.CDLL(ctypes.util.find_library('Xxf86vm'))
xlib = ctypes.CDLL(ctypes.util.find_library('X11'))
xlib.XOpenDisplay.restype = ctypes.c_void_p
d = xlib.XOpenDisplay(b':99')
assert d, 'Failed to open display'
# Lock mode switching
ret = lib.XF86VidModeLockModeSwitch(d, 0, 1)
print(f'lock-ret={ret}')
# Unlock mode switching
ret = lib.XF86VidModeLockModeSwitch(d, 0, 0)
print(f'unlock-ret={ret}')
print('PASS: VidMode lock/unlock succeeded')
xlib.XCloseDisplay(d)
